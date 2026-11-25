# Spec Delta — quality-review (validação incremental)

## ADDED Requirements

### Requirement: Digest por recuperação
O sistema SHALL calcular um `retrieval_digest` determinístico por recuperação (linha QA ou índice Cloze), cobrindo tipo de nota, linha de origem, índice Cloze, alvo, contexto visível, notas, mídia, vínculo de obrigações e evidência daquela recuperação.

#### Scenario: Digest presente no inventário
- **WHEN** `quality_prepare` é chamado sobre um lote com duas linhas QA e um Cloze de dois índices
- **THEN** o inventário traz quatro recuperações e cada uma carrega seu próprio `retrieval_digest` no formato `sha256:<64 hex>`

#### Scenario: Digest ausente em payload antigo
- **WHEN** um relatório gerado por build anterior é lido, sem nenhum `retrieval_digest`
- **THEN** a leitura é aceita e o cálculo de staleness recai no `snapshot_hash` global

### Requirement: Escopo do digest
O digest SHALL ser insensível a mudanças e a reordenações restritas a outras recuperações, e SHALL ser sensível a qualquer mudança semântica na própria recuperação.

#### Scenario: Isolamento de staleness
- **WHEN** quatro recuperações possuem revisões válidas e uma palavra de uma única resposta QA é alterada, mantendo as demais recuperações idênticas
- **THEN** exatamente três revisões continuam válidas, `reviewed_count` é três e apenas a recuperação alterada recebe `QUALITY.STALE_RETRIEVAL_DIGEST`

#### Scenario: Permutação equivalente
- **WHEN** `sources`, `obligations` e `bindings` são reordenados sem mudança semântica
- **THEN** todos os `retrieval_digest` permanecem byte-idênticos

#### Scenario: Sensibilidade ao próprio conteúdo
- **WHEN** um único caractere muda no enunciado, no alvo, no contexto visível, nas notas ou na mídia da recuperação R
- **THEN** `retrieval_digest(R)` muda e os digests de todas as demais recuperações não mudam

### Requirement: Revisão vinculada ao digest da própria recuperação
O sistema SHALL aceitar `QualityReview.retrieval_digest` opcional e usá-lo como única base de staleness quando presente; a divergência SHALL produzir `QUALITY.STALE_RETRIEVAL_DIGEST` (`Critical`) apenas para a recuperação afetada. Sem o campo, o sistema SHALL manter `QUALITY.STALE_REVIEW_SNAPSHOT`.

#### Scenario: Revisão sem digest local
- **WHEN** uma revisão traz apenas `snapshot_hash` e o lote não mudou
- **THEN** a revisão é aceita com o mesmo veredicto e os mesmos diagnósticos do contrato atual

#### Scenario: Revisão obsoleta no caminho legado
- **WHEN** uma revisão traz apenas `snapshot_hash` de um snapshot anterior do lote
- **THEN** a avaliação a rejeita com `QUALITY.STALE_REVIEW_SNAPSHOT`, sem contar como revisão válida

#### Scenario: Digest de outra recuperação
- **WHEN** a revisão da recuperação R1 declara o `retrieval_digest` de R2, ambas inalteradas
- **THEN** a revisão é rejeitada com `QUALITY.STALE_RETRIEVAL_DIGEST` e nunca computada como válida

### Requirement: Hash global informativo
O sistema SHALL manter `snapshot_hash` no relatório com papel declarado em `snapshot_hash_role`; quando o vínculo é local, divergência global SHALL NOT bloquear revisão cujo digest confere. `stale_retrieval_count` SHALL ser apenas informativo.

#### Scenario: Divergência global sem divergência local
- **WHEN** o hash global mudou por causa de outra recuperação e a revisão da recuperação R declara o digest local correto de R
- **THEN** a revisão de R é aceita e `snapshot_hash_role` é `informative`

#### Scenario: Hash global nunca é atestação
- **WHEN** estrutura, evidência e revisões declaradas satisfazem o gate com vínculo por recuperação
- **THEN** `export_authorized` e `clinical_correctness_certified` permanecem falsos e `snapshot_hash` não autoriza exportação

### Requirement: Migração aditiva e fail-closed
O contrato SHALL evoluir apenas por adição de campos opcionais, sem remover nem tornar obrigatório nenhum campo. `review_binding` SHALL aceitar `global` (default) ou `retrieval`; em `global` o resultado SHALL ser idêntico ao contrato atual.

#### Scenario: Compatibilidade de payload antigo
- **WHEN** um cliente envia o payload de hoje, sem `retrieval_digest` e sem `review_binding`
- **THEN** veredicto, diagnósticos e `reviewed_count` são os mesmos do contrato atual

#### Scenario: Modo global explícito
- **WHEN** o chamador envia `review_binding: "global"` em um lote com mudança de conteúdo
- **THEN** todas as revisões do snapshot anterior são rejeitadas com `QUALITY.STALE_REVIEW_SNAPSHOT`, preservando a semântica global

#### Scenario: Digest ausente onde é exigido
- **WHEN** `review_binding: "retrieval"` é pedido e uma revisão não traz `retrieval_digest`
- **THEN** a revisão não é aprovada, o veredicto não é `PASS` e o diagnóstico aponta a recuperação afetada
