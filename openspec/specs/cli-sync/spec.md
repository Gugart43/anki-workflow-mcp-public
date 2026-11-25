# cli-sync Specification

## Purpose

Separar operações sobre perfis Anki locais do protocolo MCP e evitar escritas implícitas ou colisões de conteúdo.

## Requirements

### Requirement: Mídia explícita e protegida
sync-media SHALL operar em dry-run por padrão e exigir apply para copiar, com lock exclusivo e checagem SHA-256 case-insensitive no plano e destino.

#### Scenario: Colisão
- **WHEN** dois arquivos convergem ao mesmo nome com bytes distintos
- **THEN** o lote retorna MEDIA.COLLISION antes de copiar

#### Scenario: Dry-run
- **WHEN** o operador não fornece apply
- **THEN** nenhum conteúdo de mídia é copiado

### Requirement: Snapshot de decks
sync-decks SHALL ler snapshot consistente da coleção sem modificar o original, normalizar hierarquia e gravar saída local atomicamente.

#### Scenario: Coleção sintética
- **WHEN** o operador fornece coleção e root explícitos
- **THEN** a saída inclui apenas root e descendentes normalizados, sem mutar a coleção
