# Arquitetura

- `server.py`: instância FastMCP e registro das ferramentas.
- `models/`: contratos Pydantic para cartões, manifesto, mídia e triagem.
- `checks/`: validações determinísticas e integração opcional de julgamento externo.
- `core/`: leitura/escrita atômica, locks, recibos, sanitização e renderização.
- `tools/`: operações MCP de validação, compilação, estado, exportação, hierarquia e mídia.

## Fluxo

1. Definir destino do deck e taxonomia; inicializar manifesto.
2. Compilar drafts estruturados ou construir CSV com os campos QA/Cloze esperados.
3. Validar formato e conteúdo técnico; revisar semântica e fontes fora da heurística.
4. Executar gates e obter recibo de conteúdo.
5. Exportar somente conteúdo correspondente ao recibo, preservando registro de IDs.

`CRITICAL` bloqueia a operação; `WARN` requer revisão. `INCOMPLETE` não é PASS. Os perfis de informação ficam fora do CSV e do hash de conteúdo. No Python legado e no modo Rust `legacy_compat`, persiste o teto de 20 cartões por lote. No Rust `strict_v2`, não há teto fixo: a contagem (QA = 1; Cloze = índices distintos) serve à revisão de cada cartão e da cobertura das obrigações, não à rejeição por quantidade. O oráculo Rust disponibiliza `review_batch_information` para perfil informativo por cartão; o parecer clínico e de fidelidade continua exigindo revisão humana.

Arquivos e caminhos são relativos ao host do processo MCP, não ao computador do cliente remoto. Sincronização de mídia remota não transfere arquivos automaticamente para outro host.
