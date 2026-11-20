# Quantidade e qualidade de informação

Separe três camadas: **recuperação obrigatória**, **contexto necessário** e **explicação opcional**. Uma frente longa pode sustentar uma única decisão; uma lacuna curta pode esconder uma lista.

`information_profile`/`information_profiles` apresentam caracteres visíveis, candidatos de enumeração e sinais consultivos, sempre com revisão semântica necessária. Em Cloze, leia cada índice como um cartão e não some todas as respostas como se fossem cobradas simultaneamente. No modo compacto, orientações comuns aparecem uma vez em `information_guidance`.

Notas curtas ou vazias são permitidas. Repetição literal sem explicação e comentários exclusivamente sobre ocorrência em prova podem gerar avisos; mencionar resposta/prova e explicar algo não basta para reprovar. Não há volume mínimo nem orçamento derivado de estatísticas de baralhos particulares. No Rust `strict_v2`, tampouco há limite fixo de 20 cartões por lote: reporte a contagem e revise cada recuperação, inclusive cada índice Cloze. `legacy_compat` mantém o teto histórico enquanto o Python permanece intacto.

A regra operacional QA de 1–2 itens permanece, sem conversão automática para Cloze: revise a unidade cobrada e mova detalhes opcionais para notas. Comprimento isolado gera aviso, não prova de sobrecarga. Pontuação não demonstra independência semântica; enumeração misturada com apostos e conjunções pode super/subcontar. Prefira listas explícitas para itens independentes e revisão editorial em construções ambíguas.

## Perfis declarativos e métricas

`DomainProfile::by_name` seleciona `med_br` (padrão) ou `general`. Ambos declaram `default_max_note_chars=1200` e `table_max_note_chars=2500`; são limiares de aviso de notas, não cotas de aprendizado. Um `max_length` explícito prevalece sobre o limite do perfil. O motor Rust compila as regexes declarativas; não existe validador de perfil JS paralelo.

`InfoPolicy` define propósito das notas, fidelidade, restrição de metadados/repetição literal e formato de lista. `ExamBoilerplateConfig` contém `enabled`, rótulos e padrões: `med_br` ativa a detecção consultiva de notas exclusivamente sobre provas; `general` a desativa. `EditorialChecklistItem` orienta o revisor, não é uma atestação automática.

O parser HTML5 real (`html5ever` em `crates/anki-validate/src/html.rs`) atua como autoridade única tanto para validação de fronteira HTML5 quanto para a extração de texto em `visible_char_count` e `compute_cognitive_metrics` (`crates/anki-validate/src/cognitive.rs`), substituindo abordagens frágeis de strip por regex/chars. O parser suprime ativamente o conteúdo interno de tags proibidas (`<script>`, `<style>`, `<iframe>`, `<object>`, `<embed>`, `<svg>`, `<math>`, `<form>`), impedindo que código ou metadados inflem a contagem visível. `note_metrics` pondera prosa e células de tabela separadamente (pesos 1.0 e 0.55); esses números descrevem apresentação e não medem diretamente carga cognitiva. `batch_information` reporta candidatos de lista em QA e Cloze, que ainda exigem julgamento.

## Segurança HTML e integridade estrutural

A fronteira de segurança rejeita conteúdo hostil ou malformado com o código `HTML.UNSAFE_CONTENT`, cobrindo:
- Tags proibidas: `<script>`, `<style>`, `<iframe>`, `<object>`, `<embed>`, `<svg>`, `<math>`, `<form>`.
- Atributos de evento inline: qualquer manipulador `on*` (como `onload`, `onclick`, `onerror`).
- Esquemas de URI perigosos: links e fontes utilizando `javascript:`, `vbscript:` ou `data:text/html`.

Além da sanitização HTML, a validação estrutural aplica verificações rigorosas:
- **Hardening de Cloze**:
  - `CLOZE.HINT_LEAKS_ANSWER`: emitido quando o hint vaza ou antecipa a resposta da lacuna, cobrindo inclusive acrônimos de 2 a 4 caracteres.
  - `CLOZE.NESTED_CLOZE`: disparado quando o hint contém um marcador de Cloze aninhado (`{{cN::`).
- **Restrição de tabelas**:
  - `TBL.CELL_TOO_LONG`: acusa células de tabela cujo conteúdo exceda 120 caracteres visíveis (`> 120`), prevenindo sobrecarga informacional em linhas tabulares.

Não confundir perfis de compatibilidade (`legacy_compat`/`strict_v2`), modo de compilação de notas (`CompileMode`) e modo editorial (`QualityMode`). `strict` é o padrão editorial; `quick` dispensa fontes/obrigações/revisões e por isso não deve ser usado como selo de revisão. Contratos de segurança e persistência estão em [arquitetura](architecture.md); as dez etapas e rubrica de sete dimensões estão no [workflow](generation-workflow.md).

PASS técnico não certifica originalidade, fidelidade às fontes, validade clínica ou eficácia pedagógica. Nenhum corpus ou material de estudo privado é distribuído com este documento.
