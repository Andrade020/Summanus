"""
Reserva uma quantidade exata de VRAM (simula um desktop mais ocupado) e fica dormindo.

Uso: python ballast.py <MiB>      (Ctrl+C ou encerrar o processo libera a memória)

Usa o cudart64_12.dll que acompanha o llama.cpp (build CUDA); não precisa instalar o CUDA.
O DLL é procurado ao lado do llama-server.exe (LLAMA_SERVER) ou em CUDART_DLL, se definida.
"""
import ctypes
import os
import sys
import time
from pathlib import Path


def achar_dll():
    if os.environ.get("CUDART_DLL"):
        return Path(os.environ["CUDART_DLL"])
    if os.environ.get("LLAMA_SERVER"):
        candidatos = sorted(Path(os.environ["LLAMA_SERVER"]).parent.glob("cudart64_*.dll"))
        if candidatos:
            return candidatos[-1]
    sys.exit("Não achei o cudart64_*.dll. Defina LLAMA_SERVER (build CUDA) ou CUDART_DLL.")


def main():
    mib = int(sys.argv[1])
    cudart = ctypes.CDLL(str(achar_dll()))
    if cudart.cudaSetDevice(0) != 0:
        sys.exit("cudaSetDevice falhou")
    ptr = ctypes.c_void_p()
    if mib > 0:
        # cudaMalloc só reserva; cudaMemset força a residência real
        if cudart.cudaMalloc(ctypes.byref(ptr), ctypes.c_size_t(mib * 1024 * 1024)) != 0:
            sys.exit("cudaMalloc falhou (VRAM insuficiente?)")
        cudart.cudaMemset(ptr, 0, ctypes.c_size_t(mib * 1024 * 1024))
        cudart.cudaDeviceSynchronize()
    print(f"reservado {mib} MiB", flush=True)
    while True:
        time.sleep(60)


if __name__ == "__main__":
    main()
