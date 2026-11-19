# Runbook: Arquivamento Seguro da Branch `feat/rust-v2-bootstrap`

Este runbook documenta o procedimento seguro para arquivar a branch de transição `feat/rust-v2-bootstrap` sem perda de histórico, mantendo uma tag anotada permanente após o merge e promoção do núcleo Rust na `main`.

> **Importante:** Este documento é um procedimento operacional padrão (SOP). Ele deve ser executado apenas por um operador humano com permissões administrativas após a aprovação da Onda 1, nunca de forma automática por subagentes ou scripts desatendidos.

---

## 1. Contexto e Auditoria

A branch histórica `feat/rust-v2-bootstrap` continha commits da fase preliminar de migração. O conteúdo relevante foi incorporado, refinado e superado na `main` através dos PRs #8 e #9.

Antes de qualquer ação, inspecione a divergência para confirmar que nenhuma alteração exclusiva foi omitida:

```bash
# 1. Sincronizar referências remotas
git fetch --all --prune

# 2. Auditar commits exclusivos na branch remota
git log --oneline main..origin/feat/rust-v2-bootstrap

# 3. Inspecionar diff agregado contra a base de cisão
git diff --stat main...origin/feat/rust-v2-bootstrap
```

---

## 2. Criação de Tag Anotada de Arquivamento (Zero Perda de Histórico)

Com a equivalência confirmada, proteja o histórico com uma tag anotada indelével:

```bash
git tag -a archive/rust-v2-bootstrap origin/feat/rust-v2-bootstrap \
  -m "Archive feat/rust-v2-bootstrap after Rust-first promotion on main (PR #8 / PR #9)"

git push origin archive/rust-v2-bootstrap
```

---

## 3. Limpeza das Referências de Branch

Após a confirmação da publicação da tag no GitHub:

```bash
# Remover a branch remota
git push origin --delete feat/rust-v2-bootstrap

# Remover a branch local (se existir)
git branch -D feat/rust-v2-bootstrap 2>/dev/null || true
```
