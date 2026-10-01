r"""
Calibra a tabela de recomendação: para cada nível de VRAM "ocupada por outros programas"
(simulado com ballast.py), testa cada configuração do modelo e mede a velocidade real.

Antes de rodar, defina (veja docs/CALIBRACAO.md):
  LLAMA_SERVER  caminho do llama-server.exe
  LLAMA_MODEL   caminho do .gguf a calibrar

Uso: python calibrate.py [--levels 0,300,600,900] [--ok-tps 9] [--cpu-only]
Uma configuração que falha em um nível é pulada nos níveis mais pesados (só piora).
Os resultados vão para calibration/resultados/.
"""
import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "resultados"
OUT.mkdir(exist_ok=True)


def _caminho(var):
    valor = os.environ.get(var)
    if not valor:
        sys.exit(f"Defina a variável de ambiente {var} (veja docs/CALIBRACAO.md).")
    return Path(valor)


SERVER = _caminho("LLAMA_SERVER")
MODEL = _caminho("LLAMA_MODEL")
PORT = 8090

Q8 = ["-ctk", "q8_0", "-ctv", "q8_0"]
OUT_CPU = ["-ot", "output.weight=CPU"]
BASE = ["-fa", "on", "-t", "8", "-np", "1", "--no-webui"]
# --n-cpu-moe = nº de camadas do modelo (todos os experts na CPU). Ajuste ao SEU modelo.
GPU = ["-ngl", "99", "--n-cpu-moe", "41"]

# nome -> argumentos (do mais exigente para o menos exigente em VRAM). Ajuste ao SEU caso.
CONFIGS = {
    "c32k":        GPU + ["-c", "32768"] + Q8,
    "c32k+outCPU": GPU + ["-c", "32768"] + Q8 + OUT_CPU,
    "c16k":        GPU + ["-c", "16384"] + Q8,
    "c16k+outCPU": GPU + ["-c", "16384"] + Q8 + OUT_CPU,
    "c8k":         GPU + ["-c", "8192"] + Q8,
    "c8k+outCPU":  GPU + ["-c", "8192"] + Q8 + OUT_CPU,
}


def vram_used():
    out = subprocess.run(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                         capture_output=True, text=True).stdout
    return int(out.split()[0])


def post(path, payload, timeout=300):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


LAST_LOAD_S = 0.0  # tempo de carregamento da última chamada a measure()


def measure(args, log_name):
    """Sobe o servidor, mede a velocidade de geração e devolve (tps, pico_vram, erro)."""
    global LAST_LOAD_S
    cmd = [str(SERVER), "-m", str(MODEL), "--port", str(PORT)] + BASE + args
    log = open(OUT / f"cal_{log_name}.log", "w", encoding="utf-8")
    proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
    peak = 0
    try:
        t0 = time.time()
        while True:
            if proc.poll() is not None:
                return None, peak, "servidor encerrou"
            if time.time() - t0 > 600:
                return None, peak, "timeout carregando"
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/health", timeout=2) as r:
                    if r.status == 200:
                        break
            except Exception:
                pass
            time.sleep(2)
        LAST_LOAD_S = time.time() - t0
        msg = {"messages": [{"role": "user", "content": "Explique em português o que é um buraco negro."}],
               "chat_template_kwargs": {"enable_thinking": False}, "temperature": 0.6}
        post("/v1/chat/completions", {**msg, "max_tokens": 8})  # aquecimento
        peak = max(peak, vram_used())
        r = post("/v1/chat/completions", {**msg, "max_tokens": 80})
        peak = max(peak, vram_used())
        return r["timings"]["predicted_per_second"], peak, ""
    finally:
        proc.terminate()
        try:
            proc.wait(20)
        except subprocess.TimeoutExpired:
            proc.kill()
        log.close()
        time.sleep(2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--levels", default="0,300,600,900", help="MiB de VRAM a reservar em cada nível")
    ap.add_argument("--out", default="calibration.jsonl")
    ap.add_argument("--ok-tps", type=float, default=9.0,
                    help="velocidade mínima para considerar que a config coube (~70%% da velocidade normal)")
    ap.add_argument("--cpu-only", action="store_true", help="mede também o modo só CPU")
    a = ap.parse_args()
    levels = [int(x) for x in a.levels.split(",")]
    out = open(OUT / a.out, "a", encoding="utf-8")
    failed = set()

    def record(rec):
        out.write(json.dumps(rec, ensure_ascii=False) + "\n")
        out.flush()
        print(json.dumps(rec, ensure_ascii=False), flush=True)

    if a.cpu_only:
        tps, peak, err = measure(["-ngl", "0", "-c", "8192"], "cpu")
        record({"cfg": "cpu-only", "extra": 0, "baseline": vram_used(), "tps": tps, "peak": peak, "err": err})

    for extra in levels:
        ballast = None
        if extra > 0:
            ballast = subprocess.Popen([sys.executable, str(HERE / "ballast.py"), str(extra)],
                                       stdout=subprocess.DEVNULL)
            time.sleep(6)
        try:
            baseline = vram_used()
            for name, args in CONFIGS.items():
                if name in failed:
                    record({"cfg": name, "extra": extra, "baseline": baseline, "tps": None, "skipped": True})
                    continue
                tps, peak, err = measure(args, f"{name}_{extra}")
                ok = tps is not None and tps >= a.ok_tps
                if not ok:
                    failed.add(name)
                record({"cfg": name, "extra": extra, "baseline": baseline, "tps": tps and round(tps, 1),
                        "peak": peak, "ok": ok, "err": err})
        finally:
            if ballast:
                ballast.terminate()
                ballast.wait()
                time.sleep(3)


if __name__ == "__main__":
    main()
