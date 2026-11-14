# Segurança e divulgação responsável

## Limites

O servidor manipula arquivos e pode ler/escrever nos destinos configurados. Execute com usuário de menor privilégio, restrinja diretórios e mantenha backups. Locks, hashes e transações reduzem falhas; não formam um sandbox contra um cliente com acesso irrestrito às ferramentas.

O transporte padrão é stdio. Para exposição remota, use autenticação, controle de acesso e criptografia; nunca publique o serviço bruto na Internet. Esta distribuição não contém endpoint de implantação ou túnel pessoal.

Integrações opcionais podem transmitir texto da fonte ao provedor configurado. O usuário é responsável por autorizar esse envio. Credenciais são lidas somente de variáveis de ambiente explicitamente fornecidas; arquivos pessoais de outros aplicativos não são descobertos automaticamente.

Não há garantia de ausência de vulnerabilidades, segredos ou riscos clínicos. Não use o resultado como decisão médica ou validação de material clínico.

## Reportar

Use a opção **Security → Report a vulnerability** do GitHub, quando habilitada. Não publique credenciais ou dados confidenciais em issues/PRs. Informe versão/SHA, impacto e reprodução mínima sintética. Se a opção privada não estiver disponível, não inclua detalhes sensíveis em canais públicos.
