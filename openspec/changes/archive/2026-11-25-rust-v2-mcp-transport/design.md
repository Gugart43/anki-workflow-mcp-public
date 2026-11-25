# Design

## Context

Ver `proposal.md`. O binário em `apps/anki-workflow-mcp/src/main.rs` implementa CLI e um oráculo que recebe um array JSON inteiro via stdin. `anki-validate` já oferece as três operações propostas; não há dependência de transporte MCP no manifesto. A suíte diferencial Python↔Rust usa o oráculo e não pode ser quebrada.

## Goals / Non-Goals

**Goals:** um servidor Rust stdio experimental, invocável explicitamente e testado como cliente MCP real, cujo catálogo corresponda exatamente às funções puras escolhidas.

**Non-Goals:** replicar todo FastMCP/Python, iniciar produção automaticamente, usar decks reais, fazer I/O transacional ou atestar qualidade clínica.

## Decisions

1. **Transporte MCP isolado por comando.** `serve` cria um loop JSON-RPC stdio assíncrono; o caminho `oracle` permanece síncrono e inalterado. Preferir o SDK Rust MCP estabelecido, após conferir versão/API em Cargo, a implementar semântica MCP manualmente. Alternativa descartada: estender o protocolo customizado do oráculo, que não é MCP.
2. **Ferramentas com parâmetros tipados.** Cada ferramenta valida entradas e chama diretamente `anki-validate`; saída estruturada conserva diagnósticos completos. Erros de argumentos/ferramentas não derrubam o processo. Alternativa descartada: aceitar qualquer JSON sem validação ou reler o stdin completo em cada chamada.
3. **Fail-closed de escopo.** Registrar apenas três ferramentas puras. Nenhuma leitura de arquivos de deck/manifesto; log e erros operacionais vão a stderr. Usar `check_csv` como terceira operação para testar diagnósticos determinísticos sem mediação Python.
4. **Teste de ponta a ponta por subprocesso.** Exercitar handshake, catálogo, chamadas boas/ruins, EOF e stdout com cliente de linhas JSON-RPC no teste de integração; comparar o payload útil às funções/oráculo existentes. Manter checks Rust e diferencial no CI.

## Risks / Trade-offs

- [Deriva do SDK MCP/API] → verificar versões e exemplos locais da crate antes da integração; executar `cargo check` antes do build.
- [Confusão entre resultado de ferramenta e resposta JSON-RPC] → documentar envoltório de `tools/call` e testá-lo pelo protocolo real.
- [Resultado grande no stdout ou travamento de stdio] → testar saída sem ruído, EOF e múltiplas requisições na mesma sessão.
- [Leitura enganosa de README/versão `0.1.0`] → apresentar `serve` como experimental e instalar a branch com revisão fixada; não prometer implantação Rust como substituta do Python.

## Migration Plan

Executar binário Rust `serve` apenas em ambiente de desenvolvimento; manter Python como padrão. Em caso de falha, interromper `serve` e usar o servidor Python sem migração de estado. Nenhuma alteração em instância remota é parte desta mudança.
