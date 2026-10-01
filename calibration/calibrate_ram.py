r"""
Calibra o efeito de pouca RAM disponível: reserva memória até sobrar X GiB "disponíveis"
(como se outros programas a estivessem usando) e mede o modelo (config que cabe na VRAM).

Uso: python calibrate_ram.py [--targets none,23,21,19,17]
     (GiB disponíveis ANTES de carregar o modelo; "none" = sem reserva)
Requer: pip install psutil, e as variáveis de calibrate.py (LLAMA_SERVER, LLAMA_MODEL).
Cuidado: escolha alvos que deixem o Windows respirar (não vá abaixo de ~10 GiB disponíveis).
"""
import argparse
import json
import subprocess
import sys
import time

import psutil

import calibrate as cal

GIB = 2 ** 30

HOLD = r"""
import sys, time
n = int(float(sys.argv[1]) * 2**30)
b = bytearray(n)
for i in range(0, n, 4096):   # tocar em cada página força a alocação física
    b[i] = 1
print("ok", flush=True)
time.sleep(3600)
"""


def avail_gib():
    return psutil.virtual_memory().available / GIB


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--targets", default="none,23,21,19,17")
    a = ap.parse_args()
    out = open(cal.OUT / "calibration_ram.jsonl", "a", encoding="utf-8")
    args = cal.GPU + ["-c", "8192"] + cal.Q8 + ["-t", "8"]

    for tgt in a.targets.split(","):
        holder = None
        if tgt != "none":
            excess = avail_gib() - float(tgt)
            if excess > 0:
                holder = subprocess.Popen([sys.executable, "-c", HOLD, f"{excess:.2f}"],
                                          stdout=subprocess.PIPE, text=True)
                holder.stdout.readline()
                time.sleep(3)
        before = avail_gib()
        try:
            t0 = time.time()
            tps, peak, err = cal.measure(args, f"ram_{tgt}")
            rec = {"avail_before_gib": round(before, 1), "target": tgt,
                   "tps": tps and round(tps, 1), "total_s": round(time.time() - t0),
                   "load_s": round(cal.LAST_LOAD_S), "avail_after_gib": round(avail_gib(), 1), "err": err}
        finally:
            if holder:
                holder.kill()
                time.sleep(3)
        out.write(json.dumps(rec) + "\n")
        out.flush()
        print(json.dumps(rec), flush=True)


if __name__ == "__main__":
    main()
