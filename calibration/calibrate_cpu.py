r"""
Calibra o efeito de CPU ocupada por outros programas: simula N processos girando em 100%
e mede a velocidade do modelo com diferentes números de threads.
Use uma configuração que caiba na VRAM com folga (aqui: c8k), para isolar o efeito da CPU.

Uso: python calibrate_cpu.py [--busy 0,4,8] [--threads 8,6,4]
     (--busy = quantos processos "queimando" CPU; a metade dos threads lógicos ~ 50% de uso)
Requer as mesmas variáveis de calibrate.py (LLAMA_SERVER, LLAMA_MODEL).
"""
import argparse
import json
import subprocess
import sys
import time

import calibrate as cal

BURN = "while True: pass"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--busy", default="0,4,8")
    ap.add_argument("--threads", default="8,6,4")
    a = ap.parse_args()
    out = open(cal.OUT / "calibration_cpu.jsonl", "a", encoding="utf-8")

    for busy in [int(x) for x in a.busy.split(",")]:
        burners = [subprocess.Popen([sys.executable, "-c", BURN]) for _ in range(busy)]
        time.sleep(4)
        try:
            for t in [int(x) for x in a.threads.split(",")]:
                args = cal.GPU + ["-c", "8192"] + cal.Q8
                # BASE já tem -t 8; o último -t da linha de comando vence
                tps, peak, err = cal.measure(args + ["-t", str(t)], f"cpu_b{busy}_t{t}")
                rec = {"busy_procs": busy, "threads": t, "tps": tps and round(tps, 1), "err": err}
                out.write(json.dumps(rec) + "\n")
                out.flush()
                print(json.dumps(rec), flush=True)
        finally:
            for b in burners:
                b.kill()
            time.sleep(2)


if __name__ == "__main__":
    main()
