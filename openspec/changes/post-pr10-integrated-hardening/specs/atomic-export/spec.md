# atomic-export

## Purpose
Garantir exportação local verificável de CSVs sem confundir integridade técnica com aprovação clínica autenticada.

## ADDED Requirements

### Requirement: Recibo e validação fail-closed
A exportação SHALL exigir PASS explícito, hash exato dos CSVs e revalidação estrutural/HTML. O recibo SHALL representar integridade, não assinatura de revisão clínica.

#### Scenario: Hash inválido
- **WHEN** o hash diverge ou o conteúdo contém HTML inseguro
- **THEN** a ferramenta rejeita antes de gravar CSVs

### Requirement: Idempotência e conflitos
O sistema SHALL serializar exportações por destino, preservar arquivos existentes em falha recuperável e rejeitar batch_id reutilizado com conteúdo distinto.

#### Scenario: Repetição íntegra
- **WHEN** o mesmo batch e hash são reenviados com arquivos íntegros
- **THEN** retorna ALREADY_EXPORTED sem sobrescrever conteúdo

#### Scenario: Falha no meio
- **WHEN** uma troca de arquivo falha após outra ter sido aplicada
- **THEN** os arquivos anteriores são restaurados ou a falha de recuperação é explicitamente preservada
