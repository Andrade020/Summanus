# Guia: calibrar um LLM local para o seu PC

Este guia conta **como a tabela de recomendação do Summanus foi construída**, passo a passo, para que você repita o processo no seu computador. O exemplo real é o **Qwen3.6-35B-A3B** rodando num PC modesto (Ryzen 5 1600, **GTX 1060 de 3 GB**, 32 GB de RAM DDR4 dual-channel, Windows 11), medido em 23-25/09/2026 com o llama.cpp `b11149`.

No fim você terá uma tabela do tipo *"com tanto de VRAM ocupada por outros programas, use esta configuração e espere esta velocidade"*, e um script que a consulta sozinho.

- Tempo total: cerca de 1h30, quase tudo esperando os testes rodarem.
- Você precisa saber abrir um PowerShell e copiar comandos. Não precisa saber programar.
- Os números deste guia são **do PC do autor**. O valor do guia é o método, não os números.

---

## 1. Conceitos que explicam tudo o que vem depois

### 1.1 Onde o modelo mora
Ao rodar um modelo, três coisas disputam memória:

| O quê | Onde fica | Cresce com |
|---|---|---|
| Pesos do modelo | RAM (o arquivo é "mapeado" na memória) e, em parte, VRAM | tamanho do arquivo |
| Cache do contexto (*KV cache*) | VRAM (ou RAM) | tamanho do contexto (`-c`) |
| Buffers de cálculo | VRAM | tamanho de lote (`-ub`) |

### 1.2 Por que um modelo de 35B roda numa placa de 3 GB
O Qwen3.6-35B-A3B é um modelo **MoE** (*mixture of experts*): tem 35B de parâmetros, mas só ~3B trabalham em cada token. O llama.cpp permite deixar os "experts" (a maior parte dos pesos) na RAM (`--n-cpu-moe`) e só a parte que trabalha em todo token (atenção etc.) na GPU. A velocidade passa a depender de RAM e CPU, e a VRAM só precisa guardar o "resto".

### 1.3 O degrau (a ideia mais importante do guia)
Quando a VRAM enche, o Windows não avisa nem dá erro: ele **passa a guardar o excesso na RAM**, que para a GPU é muito lenta. O resultado não é uma queda gradual, e sim um **degrau**:

```
configuração cabe na VRAM   ->  ~14 t/s
configuração NÃO cabe       ->  ~4 t/s   (qualquer que seja o tamanho do excesso)
```

Consequências práticas:
1. O critério de "coube ou não" é a **velocidade**, e não o pico de memória que o `nvidia-smi` mostra. Nas nossas medições, execuções lentas mostraram picos de VRAM *menores* que as rápidas, porque o excesso já tinha ido para a RAM.
2. Uma configuração que "funciona" com 32K de contexto pode virar 4 t/s só porque você abriu o navegador.

### 1.4 A GPU é compartilhada com o Windows
`dwm` (o gerenciador de janelas), o Windows Terminal, o navegador e o Steam guardam VRAM. Chamamos de **baseline** a VRAM em uso *antes* de carregar o modelo. Ela mudou bastante durante os testes (de ~210 a ~530 MiB ao longo de dois dias, e ~970 MiB numa leitura isolada). **A pergunta certa não é "cabe?", e sim "cabe com quanto de desktop?".**

### 1.5 Duas velocidades
- **Geração** (`tg`, tokens/s): a velocidade de "digitar" a resposta. É o que você sente.
- **Leitura do prompt** (`pp`, tokens/s): quão rápido ele lê o que você colou. Importa com textos longos.

---

## 2. Preparação

### 2.1 O que instalar
1. **llama.cpp**, do [releases do GitHub](https://github.com/ggml-org/llama.cpp/releases): o zip da sua GPU (para NVIDIA GTX 10xx use `win-cuda-12.4` + o zip `cudart-llama-bin-win-cuda-12.4`; o CUDA 13 não suporta mais a arquitetura Pascal). Ele traz `llama-server.exe` e `llama-bench.exe`.
2. **Python 3.9+** e `pip install psutil`.
3. **nvidia-smi**: já vem com o driver da NVIDIA.
4. Um modelo **GGUF**. Ao baixar, confira o SHA256 contra o que o Hugging Face mostra (um arquivo corrompido gera medições estranhas) e use `curl.exe -L -C -` para poder retomar downloads.

### 2.2 Variáveis usadas pelos scripts
```powershell
$env:LLAMA_SERVER = "C:\caminho\llama.cpp\cuda\llama-server.exe"
$env:LLAMA_MODEL  = "C:\caminho\models\seu-modelo.gguf"
```
Os scripts estão em [`calibration/`](../calibration). Os resultados vão para `calibration/resultados/` (ignorada pelo git).

### 2.3 Regras de ouro antes de cada rodada
- Feche navegador, jogos, Steam e qualquer coisa pesada. Um `pytest` rodando ao lado derrubou o modelo de 12 para ~1 t/s.
- Anote a VRAM em uso: `nvidia-smi --query-gpu=memory.used,memory.total --format=csv`.
- Não use o PC durante os testes.
- Pare servidores esquecidos: `Get-Process llama-server | Stop-Process -Force`.

---

## 3. Fase 0: conhecer o hardware

```powershell
nvidia-smi
Get-CimInstance Win32_PhysicalMemory | Select BankLabel,Capacity,ConfiguredClockSpeed
Get-CimInstance Win32_Processor | Select Name,NumberOfCores,NumberOfLogicalProcessors
```

O que procurar:
- **RAM em dual-channel?** Dois pentes em canais diferentes (`CHANNEL A` e `CHANNEL B`) dobram a largura de banda. No nosso caso isso acelerou a *leitura do prompt* em ~60% e não mexeu na geração (que passou a depender da CPU).
- **Tamanho do modelo vs RAM.** O arquivo precisa caber na RAM *disponível*. Regra de bolso inicial: pelo menos o tamanho do arquivo livre. O limite real se mede na seção 9.
- **Modelo MoE?** Se a GPU é pequena, MoE com poucos parâmetros ativos costuma ganhar de um modelo denso menor.

---

## 4. Fase 1: exploração rápida com `llama-bench` (minutos)

O `llama-bench` mede sem subir servidor. **Varie um parâmetro por vez** e repita cada medição (`-r 2` ou `-r 3`) para ver o ruído (`±`).

```powershell
cd C:\caminho\llama.cpp\cuda
# geração (tg) variando quantas camadas de experts ficam na CPU
.\llama-bench.exe -m $env:LLAMA_MODEL -p 0 -n 96 -r 3 -fa 1 -ngl 99 -ncmoe 36,38,40,42 -t 8
# threads
.\llama-bench.exe -m $env:LLAMA_MODEL -p 0 -n 96 -r 2 -fa 1 -ngl 99 -ncmoe 40 -t 4,6,8,12
# leitura do prompt variando o tamanho do lote
.\llama-bench.exe -m $env:LLAMA_MODEL -p 1024 -n 0 -r 2 -fa 1 -ngl 99 -ncmoe 40 -ub 256,512,1024,2048
```

O que aprendemos (modelo IQ3 em 16 GB single-channel, no começo):

| `--n-cpu-moe` | 36 | 37 | 38 | 39 | 40 |
|---|---|---|---|---|---|
| geração (t/s) | 3,6 | 3,4 | 9,9 | 10,5 | 9,5 |

Foi assim que apareceu o degrau da seção 1.3: entre 37 e 38 a velocidade triplicou.

| threads | 4 | 5 | 6 | 8 | 12 |
|---|---|---|---|---|---|
| geração (t/s) | 7,2 | 9,0 | 9,9 | 11,6 | 12,4 |

Com quantizações "IQ" o gargalo é a CPU, então usar todas as threads (com SMT) ajudou.

Leitura do prompt: `-ub 128` deu 54 t/s e `-ub 512` deu 142 t/s. Desligar o offload de operações para a GPU (`-nopo 1`) derrubou para 59 t/s.

### Ruído e cache: como não se enganar
- **A primeira leitura do arquivo é lenta** (o Windows ainda não tem o modelo em cache): vimos leitura de prompt de 40 t/s a frio e 180-220 t/s a quente, com a mesma configuração.
- **Um ponto fora da curva pede repetição, não teoria.** Um `n-cpu-moe 41` deu 4,7 t/s; ao repetir, deu 13,2. Era a primeira carga do disco.
- Nunca conclua nada de uma execução só.

---

## 5. Fase 2: validar no servidor, com contexto de verdade

O `llama-bench` usa um contexto minúsculo. O degrau depende do contexto e dos buffers, então **a validação precisa ser no `llama-server` com o contexto que você quer usar**.

Exemplo real com o Q4: no `llama-bench`, 14 t/s. No servidor com `-c 32768`, **4,2 t/s**.

O que o script `calibrate.py` faz em cada medição (a função `measure`): sobe o servidor com os argumentos, espera o `/health`, faz uma geração curta de aquecimento, mede uma de 80 tokens (lendo `timings.predicted_per_second` da resposta), registra a VRAM e derruba o servidor.

Varredura de contexto (Q4, `-ngl 99 --n-cpu-moe 41 -t 8`):

| Contexto | Cache do KV | Geração (t/s) | Pico de VRAM |
|---|---|---|---|
| 4K | f16 | 14,0-14,6 | 2857 MiB |
| 16K | f16 | 14,0-14,4 | 2873 MiB |
| 32K | f16 | **4,2** | |
| 32K | **q8** (`-ctk q8_0 -ctv q8_0`) | 12,6-14,3 | 2905 MiB |
| 32K | na RAM (`--no-kv-offload`) | 9,1-9,2 | 2777 MiB |

Para ver a queda com o contexto **cheio**, use o `llama-bench -d` (profundidade):

```powershell
.\llama-bench.exe -m $env:LLAMA_MODEL -p 2048 -n 64 -d 0,16384,30000 -r 1 -fa 1 -ngl 99 -ncmoe 41 -t 8 -ub 512 -ctk q8_0 -ctv q8_0
```
Resultado: 14,6 t/s (vazio), 12,5 (16K) e 11,0 (30K).

### As alavancas para liberar VRAM
| Alavanca | Efeito | Custo |
|---|---|---|
| `-ctk q8_0 -ctv q8_0` | cache do contexto pela metade | quase nenhum |
| `-c` menor | menos cache | menos contexto |
| `-ot "output.weight=CPU"` | camada de saída na CPU: libera ~400 MiB | ~3-4 t/s a menos |
| `--n-cpu-moe` maior | mais experts na CPU | só ajuda até cobrir todas as camadas |
| `-ub` menor | menos buffer | não ajudou neste caso |

---

## 6. Fase 3: o erro que motivou a calibração

Depois de achar uma configuração de 14 t/s com 32K de contexto, ela foi entregue como "a" configuração. No dia seguinte, o app rodava a 3,7 t/s.

O que fizemos para investigar (vale como roteiro para qualquer "ficou lento do nada"):

1. **Olhar o log do servidor.** Ele mostrava 3,7 t/s de geração, então o problema não era o app.
2. **Reproduzir sem o app**, com o mesmo comando: 4,3 t/s. Portanto a causa era o ambiente, e não o código.
3. **Ver quanta VRAM os outros programas usavam**:
   ```powershell
   (Get-Counter '\GPU Process Memory(*)\Dedicated Usage').CounterSamples |
     Where-Object { $_.CookedValue -gt 20MB } | Sort-Object CookedValue -Descending |
     Select-Object -First 8 InstanceName, @{n='MiB';e={[int]($_.CookedValue/1MB)}}
   ```
   O `InstanceName` traz `pid_NNNN`; troque por nome com `Get-Process -Id NNNN`. Resultado: `dwm` 426 MiB, Windows Terminal 203 MiB, Explorer 59 MiB.
4. **Perceber a causa**: quando a configuração foi medida, o desktop usava ~210-300 MiB, e a configuração cabia com folga quase zero. Depois o baseline subiu para 368, 460 e 531 MiB.

**Lição:** sempre registre o baseline junto com cada medição, e nunca escolha uma configuração que só funciona "no limite". Isso levou à calibração sistemática das seções seguintes.

---

## 7. Fase 4: calibrar a VRAM com um "lastro" (*ballast*)

Não dá para esperar o desktop mudar de estado por conta própria. Em vez disso, **simulamos outros programas** reservando uma quantidade exata de VRAM e testamos todas as configurações em cada nível.

### 7.1 O lastro: `ballast.py`
Um programinha que reserva N MiB de VRAM e dorme. Usa o `cudart64_12.dll` que já vem junto com o llama.cpp, via `ctypes` (`cudaMalloc` + `cudaMemset` para forçar a alocação real), então não exige instalar o CUDA.

Valide antes de usar:
```powershell
python calibration\ballast.py 400      # em outro terminal, olhe o nvidia-smi
```
No nosso teste, 400 MiB reservados subiram a VRAM em ~465 MiB (os 400 + ~65 MiB do contexto CUDA do próprio programa). Ao encerrar o processo, a memória volta.

### 7.2 A matriz: `calibrate.py`
Para cada nível de lastro (0, 300, 600 e 900 MiB) ele mede todas as configurações de `CONFIGS`.

Decisões de projeto que valem para você também:
- **Critério de "coube"**: geração `>= --ok-tps` (padrão 9 t/s). O critério é a *velocidade*, por causa do degrau da seção 1.3. Nas nossas medições as configurações que cabiam davam 9-14 t/s e as que não cabiam davam 3,8-4,4, então há um vão limpo para pôr o limite. Ajuste ao seu caso (~70% da sua velocidade normal).
- **Poda**: se uma configuração falha em um nível, é pulada nos mais pesados (só piora).
- **Medição curta**: 8 tokens de aquecimento e 80 de medida. Uma geração longa não é necessária, pois o degrau é evidente.
- **Registra o baseline** real de cada nível, com o lastro já ativo.

```powershell
cd calibration
python calibrate.py --levels 0,300,600,900 --cpu-only
```
Leva ~15 minutos. Escolha os níveis para que a configuração **mais leve** falhe no último nível; senão você não achou o limite. (`--cpu-only` mede também o modo sem GPU, útil como plano B.)

### 7.3 Resultado
"Baseline" é a VRAM em uso antes de carregar o modelo (desktop + lastro), em MiB. `ok` = coube; `X` = caiu para ~4 t/s; `-` = pulado.

| Configuração | 531 | 858 | 1095 | 1400 |
|---|---|---|---|---|
| `c32k` (32K, KV q8) | X (4,1) | - | - | - |
| `c16k` (16K, KV q8) | X (4,0) | - | - | - |
| `c8k` (8K, KV q8) | ok (12,7) | ok (12,4) | X (4,0) | - |
| `c32k+outCPU` | ok (9,6) | ok (9,2) | X (4,0) | - |
| `c16k+outCPU` | ok (9,7) | ok (10,2) | ok (9,7) | X (3,8) |
| `c8k+outCPU` | ok (10,6) | ok (9,9) | ok (10,0) | X (3,8) |
| só CPU (`-ngl 0`) | 6,4 t/s (medido só neste nível) | | | |

(`outCPU` = com `-ot output.weight=CPU`.)

Medidas fora da matriz, já no primeiro dia, completam os pontos de baixa VRAM ocupada: `c32k` funcionou a ~14 t/s com o desktop em ~210-300 MiB, e falhou (4,3) em 368 MiB; `c16k` funcionou a ~14 t/s até 460 MiB e falhou em 531.

### 7.4 Como ler
Para cada configuração há dois números:
- `ok_max`: o **maior** baseline em que ela funcionou.
- `fail_min`: o **menor** baseline em que ela falhou.

Entre os dois a resposta é **incerta**. Para estreitar o intervalo, rode mais níveis (`--levels 0,100,200,...`). O custo é tempo: cada ponto leva ~1 minuto.

| Configuração | `ok_max` (MiB) | `fail_min` (MiB) | Geração |
|---|---|---|---|
| `c32k` | ~300 | 368 | ~14 |
| `c16k` | 460 | 531 | ~14 |
| `c8k` | 858 | 1095 | ~12,5 |
| `c32k+outCPU` | 858 | 1095 | ~10 |
| `c16k+outCPU` | 1095 | 1400 | ~10 |
| `c8k+outCPU` | 1095 | 1400 | ~10 |

---

## 8. Fase 5: calibrar a CPU

Pergunta: se outros programas estão usando a CPU, quantas threads convém usar? (Threads demais brigando com outros programas pioram muito: um `pytest` ao lado chegou a derrubar o modelo a ~1 t/s.)

`calibrate_cpu.py` sobe N processos girando em 100% (`while True: pass`) e mede o modelo com 8, 6 e 4 threads, usando uma configuração que cabe na VRAM com folga (para isolar o efeito da CPU).

```powershell
python calibrate_cpu.py --busy 0,4,8 --threads 8,6,4
```

Resultado (12 threads lógicas no total):

| Processos ocupando a CPU | 8 threads | 6 threads | 4 threads |
|---|---|---|---|
| 0 (~0%) | **12,7** | 12,5 | 11,7 |
| 4 (~33%) | 11,7 | **11,8** | 11,0 |
| 8 (~67%) | 7,4 | 5,6 | **9,2** |

Conclusões: com a CPU livre ou um pouco ocupada, 8 threads é o certo. Com ~67% ocupada, **4 threads** ganha de 8 por uma boa margem, e ainda assim perde ~28% frente à CPU livre. Cada ponto é uma medição só, então os números de 8 e 6 threads em 67% (7,4 e 5,6) são ruidosos, e só a tendência é confiável.

---

## 9. Fase 6: calibrar a RAM

Pergunta: a partir de quanta RAM disponível o modelo começa a engasgar? O arquivo é mapeado na memória, e se o sistema precisa despejar páginas do modelo, a geração lê o disco no meio do caminho.

`calibrate_ram.py` reserva RAM até sobrar X GiB *disponíveis* (a métrica `available` do `psutil`, que inclui o cache que o Windows pode liberar) e **só então** carrega o modelo:

```powershell
python calibrate_ram.py --targets none,23,21,19,17
```
Cuidado: não escolha alvos que deixem o Windows sem folga (nada abaixo de ~10 GiB disponíveis).

Resultado (Q4 de 20,8 GiB, contexto 8K):

| RAM disponível antes de carregar | Geração (t/s) | Tempo de carga |
|---|---|---|
| 26,4 GiB (sem reserva) | 12,7 | 7 s |
| 23,0 GiB | 12,4 | 9 s |
| 21,0 GiB | 12,6 | 13 s |
| 19,0 GiB | 10,9 (-14%) | 15 s |
| 17,0 GiB | **5,7 (-55%)** | 29 s |

Regra que saiu daqui: com RAM disponível **igual ao tamanho do arquivo** (21 GiB para um arquivo de 20,8 GiB) o modelo ainda rodou normal. Uma parte dos pesos vai para a VRAM e não ocupa RAM, o que explica a folga. Com ~2 GiB a menos a perda é pequena (-14%) e com ~4 GiB a menos é forte (-55%). Isso vale para este modelo e esta divisão entre GPU e CPU; meça o seu.

---

## 10. Fase 7: transformar as medições em regras

Com as três tabelas você já tem tudo. Falta um script que as consulte com o estado atual do PC. O núcleo é pequeno; segue a lógica completa para você reproduzir:

```python
MARGEM_VRAM = 100   # o desktop oscila 50-100 MiB em minutos: só confie se sobrar isso
CONFORTO_TPS = 8.0  # perfil "contexto": o maior contexto que ainda dê pelo menos isto

# id, contexto, argumentos extras, velocidade medida (t/s), ok_max, fail_min  (da tabela 7.4)
CONFIGS = [
    ("c32k",        32768, "-ctk q8_0 -ctv q8_0",                              14.0,  300,  368),
    ("c16k",        16384, "-ctk q8_0 -ctv q8_0",                              14.0,  460,  531),
    ("c8k",          8192, "-ctk q8_0 -ctv q8_0",                              12.5,  858, 1095),
    ("c32k+outCPU", 32768, "-ctk q8_0 -ctv q8_0 -ot output.weight=CPU",        10.0,  858, 1095),
    ("c16k+outCPU", 16384, "-ctk q8_0 -ctv q8_0 -ot output.weight=CPU",        10.0, 1095, 1400),
    ("c8k+outCPU",   8192, "-ctk q8_0 -ctv q8_0 -ot output.weight=CPU",        10.0, 1095, 1400),
]

def status_vram(baseline, ok_max, fail_min):
    if baseline + MARGEM_VRAM <= ok_max:
        return "ok"                  # cabe, com folga
    if baseline <= ok_max:
        return "no limite"           # cabe, mas qualquer oscilação derruba
    if fail_min is None or baseline < fail_min:
        return "incerto"             # entre o último ok e a primeira falha: não medido
    return "não cabe"

def fator_cpu(cpu):                  # fração da velocidade que sobra
    return 1.00 if cpu < 20 else 0.92 if cpu < 45 else 0.72

def threads_para(cpu):
    return 8 if cpu < 45 else 6 if cpu <= 60 else 4   # o 6 é interpolação

def fator_ram(ram_gib, ok=21.0, limite=19.0):
    return 1.00 if ram_gib >= ok else 0.86 if ram_gib >= limite else 0.45
```

Como escolher:
1. Calcule `status_vram` para cada configuração e mantenha só as `ok` (se não sobrar nenhuma, aceite `no limite`; se ainda não sobrar, use só CPU).
2. Velocidade esperada = velocidade medida x `fator_cpu` x `fator_ram`.
3. **Perfil "rápida"**: a de maior velocidade esperada.
   **Perfil "contexto"** (padrão): o maior contexto entre as que dão pelo menos `CONFORTO_TPS`.
4. Gere as linhas do `config.env`: `N_CTX`, `N_THREADS`, `LLAMA_SERVER_EXTRA_ARGS`, etc.

Duas facilidades que valem a pena no script:
- `--simular vram=900,ram=19,cpu=70`: mostra o que ele recomendaria num cenário hipotético (por exemplo, com um jogo aberto), útil para conferir se as regras fazem sentido.
- `--json`: saída legível por máquina. O app em Rust do Summanus usa isso: antes de carregar o modelo, ele chama `recomendar.bat --json` e aplica a sugestão (perfil **Automático**).

**Guarde uma proteção**: o script deve se recusar a rodar se já existir um `llama-server` aberto, porque com o modelo carregado a VRAM e a RAM medidas refletem o próprio modelo, não o resto do PC.

---

## 11. Armadilhas (o que quase nos enganou)

1. **Medir a frio.** A primeira leitura do arquivo é lenta e parece um problema da configuração. Repita.
2. **Uma medição só.** Use `-r 2` ou mais e olhe o `±`. Um ponto fora da curva é ruído até prova em contrário.
3. **Concluir pelo pico de VRAM.** O degrau faz execuções lentas terem pico *menor*. Confie na velocidade.
4. **Benchmark pequeno demais.** O `llama-bench` com contexto vazio não reproduz o comportamento com 32K.
5. **Não anotar o baseline.** Sem ele, uma medição não pode ser reproduzida amanhã.
6. **Programas em segundo plano.** Terminal, navegador, Steam e testes de outros projetos mudam tudo.
7. **Servidor esquecido.** Um `llama-server` de uma rodada anterior ocupa VRAM e RAM e estraga a próxima.
8. **Windows PowerShell 5.1 e pipes.** Ele acrescenta um BOM ao passar texto de um comando para outro, o que quebra `json.load` em pipeline. Leia a saída de dentro do Python.
9. **Carga lenta.** Em disco lento ou com pouca RAM, carregar o modelo pode levar minutos; o `measure` espera até 600 s antes de desistir. Aumente se o seu disco for mais lento.
10. **Extrapolar.** A calibração vale para *esta* GPU, driver e versão do llama.cpp. Atualizou algo? Recalibre.

---

## 12. Adaptando ao seu PC

| Sua situação | O que mudar |
|---|---|
| Outra placa NVIDIA (mais ou menos VRAM) | Ajuste os níveis de `--levels` para que a configuração mais leve falhe no último, e o `--n-cpu-moe` ao número de camadas do seu modelo. Os valores de `ok_max` e `fail_min` mudam por completo: **refaça a seção 7**. |
| GPU AMD ou Intel (Vulkan) | O `ballast.py` usa CUDA e **não funciona**. Não testamos alternativas. Uma ideia: consumir VRAM abrindo programas conhecidos, ou escrever um lastro com a API da sua GPU. Para medir a VRAM em uso, os contadores do Windows (`GPU Adapter Memory`) devem servir no lugar do `nvidia-smi`. |
| Sem GPU dedicada | Pule a seção 7 (só CPU); calibre CPU (8) e RAM (9). |
| Modelo denso (não MoE) | Remova `--n-cpu-moe` e `-ot`; use `-ngl N` variando o número de camadas na GPU. O degrau da VRAM continua valendo. |
| 16 GB de RAM | O modelo precisa ser bem menor (o IQ3 de 13 GiB rodava com ~10 GiB livres). Refaça a seção 9 com os seus números. |
| Linux | O `nvidia-smi` e o `psutil` funcionam. O contador por processo (`Get-Counter`) não existe; use `nvidia-smi --query-compute-apps`. |

Um roteiro curto para o seu PC:

```powershell
# 1. Variáveis (seção 2.2), feche o resto, anote a VRAM.
# 2. Exploração rápida com o llama-bench (seção 4) para achar a ordem de grandeza.
# 3. Edite CONFIGS em calibration/calibrate.py com as suas candidatas (seção 5).
# 4. VRAM:
python calibrate.py --levels 0,300,600,900 --cpu-only
# 5. CPU:
python calibrate_cpu.py --busy 0,4,8 --threads 8,6,4
# 6. RAM:
python calibrate_ram.py --targets none,23,21,19,17
# 7. Monte as suas tabelas (7.4, 8, 9) e o script de recomendação (seção 10).
```

---

## 13. O que este guia não mediu

- A calibração é de **um único PC** e de **uma versão** do llama.cpp (`b11149`).
- O `ok_max`/`fail_min` de cada configuração vem de poucos pontos; entre eles o comportamento é incerto.
- O `c32k` sem lastro foi medido só no primeiro dia (desktop em ~210-300 MiB); o limite de ~300 MiB é aproximado.
- **Threads com CPU entre 45% e 60%** (6 threads) é interpolação. **Acima de ~67% de CPU** e **abaixo de 17 GiB de RAM** não há medições.
- O modelo **IQ3** (menor) não foi calibrado para VRAM; seus limites de RAM foram estimados por proporção ao tamanho do arquivo.
- A qualidade das respostas não foi comparada entre configurações: o teste de raciocínio usado (5 perguntas) é fácil demais para diferenciar quantizações.
- Cada ponto de CPU é **uma** medição de 80 tokens.

---

## 14. Arquivos

| Arquivo | Para quê |
|---|---|
| [`calibration/ballast.py`](../calibration/ballast.py) | Reserva VRAM exata (simula desktop ocupado) |
| [`calibration/calibrate.py`](../calibration/calibrate.py) | Matriz configurações x VRAM ocupada |
| [`calibration/calibrate_cpu.py`](../calibration/calibrate_cpu.py) | Threads x CPU ocupada |
| [`calibration/calibrate_ram.py`](../calibration/calibrate_ram.py) | Velocidade x RAM disponível |
