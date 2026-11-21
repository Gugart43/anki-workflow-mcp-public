//! MCP Handler for `anki-theme-audit-mcp`.
//!
//! Exposes:
//! - Tools: `theme_audit_css`, `theme_audit_card_html` (with read-only annotations)
//! - Prompt: `review-theme-contrast`
//! - Resource: `ankimbr://theme/presentation-v2.css`

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
        ListPromptsResult, ListResourcesResult, PaginatedRequestParams, Prompt, PromptArgument,
        PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
        Resource, ResourceContents, Role, ServerCapabilities, ServerConfig,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};

use crate::state::ServerState;
use crate::tools::theme_audit::{self, ThemeAuditCssParams, ThemeAuditHtmlParams};

/// MCP server handler managing state, tools, prompts, and resources.
#[derive(Debug, Clone)]
pub struct McpHandler {
    state: ServerState,
    tool_router: ToolRouter<Self>,
}

impl Default for McpHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl McpHandler {
    pub fn new() -> Self {
        Self {
            state: ServerState::new(),
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl McpHandler {
    #[tool(
        name = "theme_audit_css",
        description = "Audita CSS AnkiMBR Light/Dark: detecta prefers-color-scheme sem opt-in, assimetria de tokens, !important e contraste WCAG 2.1",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn theme_audit_css(
        &self,
        params: Parameters<ThemeAuditCssParams>,
    ) -> Result<String, String> {
        theme_audit::audit_css(params).await
    }

    #[tool(
        name = "theme_audit_card_html",
        description = "Audita HTML de cartão Anki contra estilos inline que quebram tema claro/escuro (color, background, bgcolor, font, svg)",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn theme_audit_card_html(
        &self,
        params: Parameters<ThemeAuditHtmlParams>,
    ) -> Result<String, String> {
        theme_audit::audit_html(params).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "anki-theme-audit-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Servidor MCP de auditoria de tema AnkiMBR Light/Dark, WCAG 2.1 e recursos CSS.",
        )
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let prompt = Prompt::new(
            "review-theme-contrast",
            Some("Revisa contraste WCAG 2.1 e isolamento Light/Dark do CSS AnkiMBR"),
            Some(vec![
                PromptArgument::new("target_level")
                    .with_title("Nível de conformidade WCAG")
                    .with_description("AA ou AAA")
                    .with_required(false),
            ]),
        )
        .with_title("Revisão de Contraste AnkiMBR");

        Ok(ListPromptsResult::with_all_items(vec![prompt]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let _ = self.state.record_audit().await;
        match request.name.as_str() {
            "review-theme-contrast" => {
                let level = request
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("target_level"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("AA");

                let result = GetPromptResult::new(vec![PromptMessage::new_text(
                    Role::User,
                    format!(
                        "Execute theme_audit_css e verifique conformidade WCAG {level} para todos os pares Light e Dark sem prefers-color-scheme solto."
                    ),
                )])
                .with_description("Prompt de revisão WCAG Light/Dark");

                Ok(result.into())
            }
            _ => Err(McpError::invalid_params("Unknown prompt", None)),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resource = Resource::new(
            "ankimbr://theme/presentation-v2.css",
            "AnkiMBR Presentation v2 CSS",
        )
        .with_title("AnkiMBR Presentation v2 CSS")
        .with_description("Folha de estilo Light/Dark v2 verificada em WCAG 2.1")
        .with_mime_type("text/css");

        Ok(ListResourcesResult::with_all_items(vec![resource]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        match request.uri.as_str() {
            "ankimbr://theme/presentation-v2.css" => {
                let contents = ResourceContents::text(
                    include_str!("../../../docs/reference-theme/ankimbr-presentation-v2.css"),
                    &request.uri,
                )
                .with_mime_type("text/css");

                let result = ReadResourceResult::new(vec![contents]);
                Ok(result.into())
            }
            _ => Err(McpError::resource_not_found(request.uri, None)),
        }
    }
}
