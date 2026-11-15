use std::collections::HashMap;
use std::fmt::Write;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowNode {
    pub id: String,
    pub texto: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowEdge {
    pub origem: String,
    pub destino: String,
    pub rotulo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteFlow {
    pub inicio: String,
    pub nos: Vec<FlowNode>,
    #[serde(default)]
    pub arestas: Vec<FlowEdge>,
}

fn sanitize_text(value: &str) -> String {
    crate::sanitize::sanitize_field(value, false, false).0
}

pub fn render_note_flow(flow: &NoteFlow) -> String {
    let mut nodes: Vec<&FlowNode> = flow.nos.iter().collect();
    nodes.sort_by_key(|n| n.id != flow.inicio);

    let mut numbers: HashMap<&str, usize> = HashMap::with_capacity(nodes.len());
    let mut labels: HashMap<&str, &str> = HashMap::with_capacity(nodes.len());
    for (idx, node) in nodes.iter().enumerate() {
        numbers.insert(node.id.as_str(), idx + 1);
        labels.insert(node.id.as_str(), node.texto.as_str());
    }

    let mut out = String::from("<ol>");
    for node in &nodes {
        let node_text = sanitize_text(&node.texto);
        let _ = write!(out, "<li><b>{node_text}</b>");

        let edges: Vec<&FlowEdge> = flow
            .arestas
            .iter()
            .filter(|edge| edge.origem == node.id)
            .collect();

        if !edges.is_empty() {
            out.push_str("<ul>");
            for edge in edges {
                let edge_label = sanitize_text(&edge.rotulo);
                let target_num = numbers.get(edge.destino.as_str()).copied().unwrap_or(0);
                let target_text =
                    sanitize_text(labels.get(edge.destino.as_str()).copied().unwrap_or(""));
                let _ = write!(out, "<li>{edge_label} → {target_num}. {target_text}</li>");
            }
            out.push_str("</ul>");
        }

        out.push_str("</li>");
    }
    out.push_str("</ol>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sorted_flow_with_inicio_first_and_preserves_order() {
        let flow = NoteFlow {
            inicio: String::from("n1"),
            nos: vec![
                FlowNode {
                    id: String::from("n2"),
                    texto: String::from("Segundo passo"),
                },
                FlowNode {
                    id: String::from("n1"),
                    texto: String::from("Início"),
                },
                FlowNode {
                    id: String::from("n3"),
                    texto: String::from("Terceiro passo"),
                },
            ],
            arestas: vec![
                FlowEdge {
                    origem: String::from("n1"),
                    destino: String::from("n2"),
                    rotulo: String::from("Sim"),
                },
                FlowEdge {
                    origem: String::from("n1"),
                    destino: String::from("n3"),
                    rotulo: String::from("Não"),
                },
            ],
        };

        let html = render_note_flow(&flow);
        assert_eq!(
            html,
            "<ol><li><b>Início</b><ul><li>Sim → 2. Segundo passo</li><li>Não → 3. Terceiro passo</li></ul></li><li><b>Segundo passo</b></li><li><b>Terceiro passo</b></li></ol>"
        );
    }

    #[test]
    fn renders_empty_flow_as_empty_ordered_list() {
        let flow = NoteFlow {
            inicio: String::new(),
            nos: Vec::new(),
            arestas: Vec::new(),
        };

        assert_eq!(render_note_flow(&flow), "<ol></ol>");
    }

    #[test]
    fn renders_single_node_without_edges_and_omits_nested_list() {
        let flow = NoteFlow {
            inicio: String::from("only"),
            nos: vec![FlowNode {
                id: String::from("only"),
                texto: String::from("Único nó"),
            }],
            arestas: Vec::new(),
        };

        let html = render_note_flow(&flow);
        assert_eq!(html, "<ol><li><b>Único nó</b></li></ol>");
        assert!(!html.contains("<ul>"));
    }
}
