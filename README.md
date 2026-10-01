# Summanus

<img src="assets/summanus-mark.svg" alt="Símbolo do Summanus" width="96">

Leia também o artigo [Como fiz uma IA local rodar bem numa placa de vídeo de 3 GB](docs/IA_LOCAL_PC_FRACO.md), que conta os testes e as decisões de configuração em linguagem acessível.

**Summanus** é um app desktop em Rust para conversar com modelos GGUF locais. O nome vem da [divindade romana associada ao trovão noturno](https://ora.ox.ac.uk/objects/uuid:0a487637-5cef-4436-b58e-455f84c0331e). A identidade visual combina azul profundo, coral e lilás, com [símbolo vetorial](assets/summanus-mark.svg), ilustração própria e componentes arredondados. O motor de inferência continua sendo o `llama-server` oficial do `llama.cpp`.

## Funcionalidades

- Respostas em streaming com Markdown, títulos, listas, texto em destaque, código inline e blocos com crases triplas. Fórmulas LaTeX são renderizadas em Rust a partir de `$$...$$`, `\[...\]`, `\(...\)`, `$...$` e `[/.../]`, com botão para copiar a expressão.
- Botão para copiar a resposta inteira e botão separado para cada bloco de código.
- Histórico de conversas salvo automaticamente em `data/state.json`, com busca, seleção, confirmação de exclusão e exportação em Markdown, JSON ou texto.
- Sugestões de início para explorar ideias, escrever e programar; `Ctrl+N` abre uma nova conversa.
- Mapa de contextos: salve recortes por intervalo de mensagens, dê título e anotação, edite o resumo ou peça ao modelo para gerá-lo. Marque resumos como referências para os próximos envios. Quando existe um resumo do início da conversa, o mais recente substitui automaticamente as mensagens antigas nos próximos envios. A conversa completa só volta a ser enviada ao escolher **Usar conversa inteira**. As mensagens originais continuam visíveis e exportáveis.
- Barra de contexto com quantidade de tokens e capacidade configurada. Com o modelo carregado, o app consulta o tokenizador do servidor; durante a digitação e antes da resposta, mostra uma estimativa provisória.
- Antes de enviar, o app espera a contagem real do tokenizador. Quando sobra uma margem pequena, de até 5% do contexto, pausa o envio e oferece compactar o histórico ou reiniciar o modelo com contexto maior. Se houver histórico seguro para resumir e você não escolher em 30 segundos, compacta automaticamente, mede de novo e continua. Texto e anexos permanecem no compositor se a medição ou o resumo falhar.
- Importação local de texto e código: escolha o arquivo inteiro, um intervalo de linhas ou uma função de um `.py` (inclusive métodos e funções com decoradores). A prévia permite percorrer o corpo inteiro da função. Dados `data:...;base64,...` são removidos de HTML. O app avisa antes de anexar trechos grandes, sem cortá-los automaticamente.
- Modo raciocínio com painel recolhível; controles de temperatura, Top P, penalidade de repetição, presence penalty, limite de tokens e hardware.
- Velocidade real em tokens/s destacada abaixo da barra de contexto e salva em cada resposta, conforme a medição final do servidor; botão para interromper a geração.
- Cache opcional que distingue modelo, histórico e parâmetros. Desligado por padrão porque respostas amostradas podem variar.
- O servidor permanece carregado entre mensagens. A aplicação usa um único slot e solicita reaproveitamento do prompt ao `llama-server`.

## Instalação no Windows

1. Instale [Rust](https://rustup.rs/) e um compilador C para Windows: Visual Studio Build Tools com C++ (MSVC) ou MSYS2 com `mingw-w64-x86_64-gcc`.
2. Baixe o `llama-server.exe` em [llama.cpp Releases](https://github.com/ggml-org/llama.cpp/releases). Para NVIDIA GTX 10xx, use o build CUDA 12.4 e o pacote `cudart` correspondente. Escolha um build compatível com o seu hardware.
3. Copie `config.example.env` para `config.env` e ajuste `LLAMA_SERVER_PATH` e `MODEL_PATH`.
4. Execute `iniciar.bat`. Na primeira execução ele compila o app; depois abre o executável já criado. Para compilar manualmente, use `build.bat`.

O app carrega automaticamente o modelo do perfil escolhido, inclusive se você abrir o `.exe` diretamente. No perfil **Automático**, usa a indicação do recomendador. `MODEL_PATH` é o modelo base dos perfis Q4 e o fallback se o recomendador falhar. Você também pode clicar em **Trocar modelo**; a escolha fica salva no modo personalizado. Use `iniciar.bat --model caminho\modelo.gguf` para selecionar outro modelo temporariamente.
Use `iniciar.bat --no-auto-model` para abrir apenas a interface sem ocupar a GPU.

## Perfis de execução

Antes de carregar o modelo, o app executa `%USERPROFILE%\Desktop\LLM\recomendar.bat --json --ignorar-servidor`. A recomendação usa a VRAM, a RAM e a CPU livres naquele momento. O `.bat` não pausa quando recebe `--json`, para permitir a chamada pelo app. O perfil **Automático** é o padrão; a sugestão e os recursos medidos aparecem na janela **Perfis**. Se o script falhar, o app usa os valores de `config.env`. Clique em **Reavaliar PC e reiniciar** após abrir ou fechar programas pesados. A reavaliação descarrega o modelo antes de medir novamente.

Se o script estiver em outro lugar, defina `SUMMANUS_RECOMMENDER_PATH` com o caminho completo do `.bat`. A variável antiga `LOCAL_LLM_RECOMMENDER_PATH` continua aceita para preservar configurações existentes. O app mantém a escolha manual de perfil entre aberturas e continua executando o recomendador para mostrar a sugestão atual.

| Perfil | Modelo e configuração |
| --- | --- |
| A · Rápido | Q4, 16K, `-ctk q8_0 -ctv q8_0` |
| B · Contexto livre | Q4, 32K, `-ctk q8_0 -ctv q8_0 -ot output.weight=CPU` |
| C · Atual | Q4, 32K, `-ctk q8_0 -ctv q8_0` |
| Q3 · Ágil | IQ3, 32K, `N_CPU_MOE=39`, cache Q8 e `--load-mode none` |
| Q3 · Poupar VRAM | IQ3, 16K, saída na CPU e `N_CPU_MOE=41` |
| Paciente | Q4, 48K, só CPU, raciocínio ligado e limite inicial de pelo menos 8192 tokens de resposta |

O perfil escolhido fica salvo em `data/state.json`; o arquivo `config.env` continua como base e não é sobrescrito. Todos os perfis com arquivo de modelo presente podem ser selecionados manualmente. O app avisa em vermelho quando o uso atual de RAM/VRAM indica lentidão ou risco de falha ao carregar. Os valores de tokens por segundo mostrados pelo recomendador são estimativas, não uma medição do app. Trocar de perfil reinicia o servidor, mas mantém as conversas. Escolher um arquivo em **Trocar modelo** passa para o modo personalizado.

No compositor, **Enviar · Ctrl+Enter** indica o atalho de envio. A caixa **Raciocínio** controla apenas o modo de resposta. O texto digitado usa a cor clara da interface.

Na importação, cada arquivo de origem pode ter até 8 MB. Acima de 1000 linhas, 60 mil caracteres ou da capacidade estimada do contexto, o app pede confirmação para anexar o trecho inteiro. Um arquivo novo que sozinho não cabe no contexto não pode ser compactado automaticamente sem perder detalhes; escolha um trecho menor ou amplie o contexto. Os trechos são cópias do conteúdo no momento do envio, sem leitura automática posterior do arquivo. O resumo por IA também pode ser solicitado manualmente em **Resumir com IA**.

## Desempenho

As opções `N_GPU_LAYERS`, `N_CPU_MOE`, `N_CTX`, `N_THREADS` e `N_THREADS_BATCH` do `config.env` são encaminhadas ao `llama-server`. A interface permite ajustá-las durante a sessão e recarregar o modelo. Um contexto menor reduz o uso de memória; mais camadas na GPU podem acelerar a geração se houver VRAM suficiente. Em modelos MoE, `N_CPU_MOE` ajuda a manter experts na RAM quando a VRAM é limitada. Os melhores valores dependem do computador e do modelo.

`ENABLE_THINKING` define o estado inicial do raciocínio em cada abertura. Com ele desligado, o presence penalty começa em `1.5` (ou no valor de `DEFAULT_PRESENCE_PENALTY`, se definido). O ajuste também está disponível na interface e é enviado à API do servidor.

Coloque `LLAMA_SERVER_EXTRA_ARGS` entre aspas quando tiver espaços, por exemplo `LLAMA_SERVER_EXTRA_ARGS="-ctk q8_0 -ctv q8_0"`. O app mostra um erro se não conseguir ler o arquivo de configuração.

O Rust controla a interface, o histórico, o cache e o cliente HTTP. O cálculo dos tokens continua no `llama.cpp`; trocar a interface por Rust não altera diretamente a velocidade matemática do modelo. O reaproveitamento do prompt pode reduzir o tempo para começar respostas seguintes, dependendo do servidor e do histórico.

O servidor é iniciado com `--no-context-shift`. A compactação troca o histórico enviado ao modelo por um resumo, enquanto as mensagens originais continuam salvas e podem voltar ao contexto com **Usar conversa inteira**. Se a ampliação do contexto falhar ao carregar, o app restaura a configuração anterior e tenta compactar o histórico.

## Arquivos

- `config.env`: configuração local, não versionada.
- `data/state.json`: histórico, configurações de geração e último modelo escolhido.
- `logs/llama-server.log`: saída do servidor para diagnóstico.
- `cache/`: respostas salvas quando o cache está ativado.

O app Python anterior continua no repositório para referência, mas `iniciar.bat` abre a versão Rust.
