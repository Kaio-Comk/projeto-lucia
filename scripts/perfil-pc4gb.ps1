# Projeto Lucia - perfil para o PC alvo (GTX 1650 SUPER 4 GB, Xeon 4c/8t, 16 GB RAM).
# Usado por executar.ps1, aquecer.ps1 e pelo atalho de jogo: as chaves que mudam a
# traducao de shaders precisam ser IGUAIS no aquecimento e no jogo, senao o cache nao serve.
# Origem de cada escolha entre parenteses. Para testar sem uma chave: -Env @{ CHAVE = '' }.

$PerfilPc4gb = [ordered]@{
    # --- Video ---
    VICTORY_DISPLAY_MODE          = "1280x720"  # o titulo ignora e renderiza no proprio tamanho (upstream run 303)

    # --- Memoria de video: 4 GB ---
    VICTORY_MIP_CAP               = "2"         # corta os 2 niveis maiores de texturas >= 2048 (Lucia, -410 MB com 1)
    VICTORY_TEXTURE_BUDGET_MB     = "2048"      # teto das copias de texturas que o titulo enche
    VICTORY_CARD_BUFFERS_MB       = "512"       # copias de buffers na placa
    VICTORY_CARD_WINDOWS_MB       = "256"       # janelas na placa: menos aqui = mais textura (upstream run 295)

    # --- Ray tracing: sem RT cores e o construtor cai em 30-40 s (upstream README 27/11) ---
    VICTORY_BVH_BUILDER           = "0"         # desligado: testes de raio respondem "sem acerto"

    # --- Wave64 numa GPU de 32 lanes ---
    VICTORY_WAVE64_PAIRED         = "1"         # forma pareada do upstream para compute wave64 recusado
    VICTORY_WAVE_PAIRS_ONLY       = "5dc3b25986622949,6f225b641c5b0708"  # agua (upstream)
    VICTORY_WAVE_PAIRS_FP32       = "non-ieee-rne-ftz"                     # agua (upstream)

    # --- CPU (Xeon de 4 nucleos) ---
    VICTORY_HEAP_WORKER           = "1"         # passe de texturas na propria thread (upstream run 309: 96-116 ms -> 33 ms)
    VICTORY_CARD_HASH_WIDE        = "1"         # hash na placa por grupo (Trilha B)

    # --- Jogo ---
    VICTORY_TEXTURE_BACK          = "1"         # sem isto o barco afunda na 1a missao (upstream)
    VICTORY_AMBIENT               = "0.6"       # luz ambiente minima onde o RT nao roda (VItoriaBr/Belobug)
    VICTORY_D3D12_DRED            = "faults"    # diagnostico leve de perda da placa
}
