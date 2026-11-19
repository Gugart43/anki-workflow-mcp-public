use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::note_flow::{FlowEdge, FlowNode, NoteFlow};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawFlow {
    pub inicio: String,
    pub nos: Vec<FlowNode>,
    #[serde(default)]
    pub arestas: Vec<FlowEdge>,
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum FlowError {
    #[error("o fluxo deve conter ao menos um nó")]
    EmptyNodes,
    #[error("o id de um nó não pode ser vazio")]
    EmptyNodeId,
    #[error("o texto do nó '{0}' não pode ser vazio")]
    EmptyNodeText(String),
    #[error("id de nó duplicado: '{0}'")]
    DuplicateNodeId(String),
    #[error("nó inicial '{0}' não encontrado")]
    MissingStartNode(String),
    #[error("aresta '{edge_from}' -> '{edge_to}' referencia nó inexistente: '{missing}'")]
    MissingEdgeNode {
        edge_from: String,
        edge_to: String,
        missing: String,
    },
    #[error("ciclo detectado envolvendo o nó '{0}'")]
    CycleDetected(String),
    #[error("nó inalcançável a partir do início: '{0}'")]
    UnreachableNode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum FlowTopology {
    Linear,
    ShallowTree,
    ConvergentDag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawFlow", into = "RawFlow")]
#[schemars(with = "RawFlow")]
pub struct ValidatedFlow {
    inicio: String,
    nos: Vec<FlowNode>,
    arestas: Vec<FlowEdge>,
}

impl ValidatedFlow {
    #[must_use]
    pub fn inicio(&self) -> &str {
        &self.inicio
    }

    #[must_use]
    pub fn nos(&self) -> &[FlowNode] {
        &self.nos
    }

    #[must_use]
    pub fn arestas(&self) -> &[FlowEdge] {
        &self.arestas
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nos.len()
    }

    #[must_use]
    pub fn max_depth(&self) -> usize {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::with_capacity(self.nos.len());
        for edge in &self.arestas {
            adj.entry(edge.origem.as_str())
                .or_default()
                .push(edge.destino.as_str());
        }

        let mut depth: HashMap<&str, usize> = HashMap::with_capacity(self.nos.len());
        depth.insert(self.inicio.as_str(), 0);

        let mut max_d = 0usize;
        for u in self.topological_order() {
            let d_u = depth.get(u).copied().unwrap_or(0);
            if let Some(neighbors) = adj.get(u) {
                for &v in neighbors {
                    let candidate = d_u + 1;
                    let entry = depth.entry(v).or_insert(0);
                    if candidate > *entry {
                        *entry = candidate;
                        if candidate > max_d {
                            max_d = candidate;
                        }
                    }
                }
            }
        }
        max_d
    }

    #[must_use]
    pub fn topology(&self) -> FlowTopology {
        let mut in_degree: HashMap<&str, usize> = HashMap::with_capacity(self.nos.len());
        let mut out_degree: HashMap<&str, usize> = HashMap::with_capacity(self.nos.len());

        for edge in &self.arestas {
            *out_degree.entry(edge.origem.as_str()).or_insert(0) += 1;
            *in_degree.entry(edge.destino.as_str()).or_insert(0) += 1;
        }

        let max_in = in_degree.values().copied().max().unwrap_or(0);
        let max_out = out_degree.values().copied().max().unwrap_or(0);

        if max_in <= 1 && max_out <= 1 {
            FlowTopology::Linear
        } else if max_in <= 1 && self.max_depth() <= 3 {
            FlowTopology::ShallowTree
        } else {
            FlowTopology::ConvergentDag
        }
    }

    pub fn topological_order(&self) -> Vec<&str> {
        let mut in_degree: HashMap<&str, usize> = HashMap::with_capacity(self.nos.len());
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::with_capacity(self.nos.len());

        for node in &self.nos {
            in_degree.insert(node.id.as_str(), 0);
            adj.insert(node.id.as_str(), Vec::new());
        }
        for edge in &self.arestas {
            if let Some(deg) = in_degree.get_mut(edge.destino.as_str()) {
                *deg += 1;
            }
            if let Some(neighbors) = adj.get_mut(edge.origem.as_str()) {
                neighbors.push(edge.destino.as_str());
            }
        }

        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(self.inicio.as_str());

        let mut order = Vec::with_capacity(self.nos.len());
        while let Some(u) = queue.pop_front() {
            order.push(u);
            if let Some(neighbors) = adj.get(u) {
                for &v in neighbors {
                    if let Some(deg) = in_degree.get_mut(v) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(v);
                        }
                    }
                }
            }
        }
        order
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

fn validate_raw(raw: &RawFlow) -> Result<(), FlowError> {
    if raw.nos.is_empty() {
        return Err(FlowError::EmptyNodes);
    }

    let mut node_ids: HashSet<&str> = HashSet::with_capacity(raw.nos.len());
    for node in &raw.nos {
        if node.id.trim().is_empty() {
            return Err(FlowError::EmptyNodeId);
        }
        if node.texto.trim().is_empty() {
            return Err(FlowError::EmptyNodeText(node.id.clone()));
        }
        if !node_ids.insert(node.id.as_str()) {
            return Err(FlowError::DuplicateNodeId(node.id.clone()));
        }
    }

    if raw.inicio.trim().is_empty() || !node_ids.contains(raw.inicio.as_str()) {
        return Err(FlowError::MissingStartNode(raw.inicio.clone()));
    }

    let mut adj: HashMap<&str, Vec<&str>> = HashMap::with_capacity(raw.nos.len());
    for node in &raw.nos {
        adj.insert(node.id.as_str(), Vec::new());
    }

    for edge in &raw.arestas {
        if !node_ids.contains(edge.origem.as_str()) {
            return Err(FlowError::MissingEdgeNode {
                edge_from: edge.origem.clone(),
                edge_to: edge.destino.clone(),
                missing: edge.origem.clone(),
            });
        }
        if !node_ids.contains(edge.destino.as_str()) {
            return Err(FlowError::MissingEdgeNode {
                edge_from: edge.origem.clone(),
                edge_to: edge.destino.clone(),
                missing: edge.destino.clone(),
            });
        }
        if let Some(neighbors) = adj.get_mut(edge.origem.as_str()) {
            neighbors.push(edge.destino.as_str());
        }
    }

    // DFS iterativa de 3 cores (garante DAG sem ciclos, sem recursão).
    let mut colors: HashMap<&str, Color> = raw
        .nos
        .iter()
        .map(|n| (n.id.as_str(), Color::White))
        .collect();

    let mut roots: Vec<&str> = Vec::with_capacity(raw.nos.len());
    roots.push(raw.inicio.as_str());
    for node in &raw.nos {
        if node.id != raw.inicio {
            roots.push(node.id.as_str());
        }
    }

    for start in roots {
        if colors.get(start).copied() != Some(Color::White) {
            continue;
        }
        let mut stack: Vec<(&str, bool)> = vec![(start, false)];
        while let Some((u, exiting)) = stack.pop() {
            if exiting {
                colors.insert(u, Color::Black);
                continue;
            }
            match colors.get(u).copied().unwrap_or(Color::White) {
                Color::Black | Color::Gray => continue,
                Color::White => {
                    colors.insert(u, Color::Gray);
                    stack.push((u, true));
                    if let Some(neighbors) = adj.get(u) {
                        for &v in neighbors.iter().rev() {
                            match colors.get(v).copied().unwrap_or(Color::White) {
                                Color::Gray => {
                                    return Err(FlowError::CycleDetected(v.to_string()));
                                }
                                Color::White => stack.push((v, false)),
                                Color::Black => {}
                            }
                        }
                    }
                }
            }
        }
    }

    // BFS a partir de `inicio` (rejeita nós órfãos/inalcançáveis).
    let mut reachable: HashSet<&str> = HashSet::with_capacity(raw.nos.len());
    let mut queue: VecDeque<&str> = VecDeque::new();
    reachable.insert(raw.inicio.as_str());
    queue.push_back(raw.inicio.as_str());

    while let Some(u) = queue.pop_front() {
        if let Some(neighbors) = adj.get(u) {
            for &v in neighbors {
                if reachable.insert(v) {
                    queue.push_back(v);
                }
            }
        }
    }

    for node in &raw.nos {
        if !reachable.contains(node.id.as_str()) {
            return Err(FlowError::UnreachableNode(node.id.clone()));
        }
    }

    Ok(())
}

impl TryFrom<RawFlow> for ValidatedFlow {
    type Error = FlowError;

    fn try_from(raw: RawFlow) -> Result<Self, Self::Error> {
        validate_raw(&raw)?;
        let mut seen_edges = HashSet::with_capacity(raw.arestas.len());
        let mut arestas = Vec::with_capacity(raw.arestas.len());
        for edge in raw.arestas {
            let key = (
                edge.origem.clone(),
                edge.destino.clone(),
                edge.rotulo.trim().to_owned(),
            );
            if seen_edges.insert(key) {
                arestas.push(edge);
            }
        }
        Ok(Self {
            inicio: raw.inicio,
            nos: raw.nos,
            arestas,
        })
    }
}

impl TryFrom<&NoteFlow> for ValidatedFlow {
    type Error = FlowError;

    fn try_from(flow: &NoteFlow) -> Result<Self, Self::Error> {
        Self::try_from(RawFlow {
            inicio: flow.inicio.clone(),
            nos: flow.nos.clone(),
            arestas: flow.arestas.clone(),
        })
    }
}

impl From<ValidatedFlow> for RawFlow {
    fn from(flow: ValidatedFlow) -> Self {
        Self {
            inicio: flow.inicio,
            nos: flow.nos,
            arestas: flow.arestas,
        }
    }
}

fn escape_html_csv(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '|' => escaped.push_str("&#124;"),
            '\n' => escaped.push(' '),
            '\r' => {}
            other => escaped.push(other),
        }
    }
    escaped
}

fn write_title(out: &mut String, titulo: Option<&str>) {
    if let Some(t) = titulo.map(str::trim).filter(|t| !t.is_empty()) {
        let _ = write!(out, "<p><b>{}</b></p>", escape_html_csv(t));
    }
}

fn render_linear(
    flow: &ValidatedFlow,
    titulo: Option<&str>,
    node_map: &HashMap<&str, &FlowNode>,
    edges_from: &HashMap<&str, Vec<&FlowEdge>>,
) -> String {
    let mut out = String::from("<section class=\"anki-steps\">");
    write_title(&mut out, titulo);
    out.push_str("<ol>");

    let mut current: Option<(&str, Option<&str>)> = Some((flow.inicio.as_str(), None));
    while let Some((node_id, incoming_label)) = current {
        let text = node_map
            .get(node_id)
            .map(|n| escape_html_csv(n.texto.trim()))
            .unwrap_or_default();

        match incoming_label {
            Some(label) => {
                let _ = write!(out, "<li><b>{}:</b> {}</li>", escape_html_csv(label), text);
            }
            None => {
                let _ = write!(out, "<li>{text}</li>");
            }
        }

        current = edges_from
            .get(node_id)
            .and_then(|edges| edges.first())
            .map(|edge| {
                let label = Some(edge.rotulo.trim()).filter(|l| !l.is_empty());
                (edge.destino.as_str(), label)
            });
    }

    out.push_str("</ol></section>");
    out
}

fn render_shallow_tree_node(
    node_id: &str,
    incoming_label: Option<&str>,
    node_map: &HashMap<&str, &FlowNode>,
    edges_from: &HashMap<&str, Vec<&FlowEdge>>,
    out: &mut String,
) {
    let text = node_map
        .get(node_id)
        .map(|n| escape_html_csv(n.texto.trim()))
        .unwrap_or_default();
    let children = edges_from.get(node_id).map(Vec::as_slice).unwrap_or(&[]);

    out.push_str("<li>");
    match incoming_label {
        Some(label) => {
            let _ = write!(out, "<b>{} →</b> {}", escape_html_csv(label), text);
        }
        None => {
            if children.is_empty() {
                out.push_str(&text);
            } else {
                let _ = write!(out, "<b>{text}</b>");
            }
        }
    }

    if !children.is_empty() {
        out.push_str("<ul>");
        for edge in children {
            let label = Some(edge.rotulo.trim()).filter(|l| !l.is_empty());
            render_shallow_tree_node(edge.destino.as_str(), label, node_map, edges_from, out);
        }
        out.push_str("</ul>");
    }
    out.push_str("</li>");
}

fn render_shallow_tree(
    flow: &ValidatedFlow,
    titulo: Option<&str>,
    node_map: &HashMap<&str, &FlowNode>,
    edges_from: &HashMap<&str, Vec<&FlowEdge>>,
) -> String {
    let mut out = String::from("<section class=\"anki-flow\">");
    write_title(&mut out, titulo);
    out.push_str("<ul>");
    render_shallow_tree_node(flow.inicio.as_str(), None, node_map, edges_from, &mut out);
    out.push_str("</ul></section>");
    out
}

fn render_convergent_dag(
    flow: &ValidatedFlow,
    titulo: Option<&str>,
    node_map: &HashMap<&str, &FlowNode>,
    edges_from: &HashMap<&str, Vec<&FlowEdge>>,
) -> String {
    let order = flow.topological_order();
    let step_by_id: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(idx, &id)| (id, idx + 1))
        .collect();

    let mut out = String::from("<section class=\"anki-flow\">");
    write_title(&mut out, titulo);
    out.push_str("<ol>");

    for node_id in order {
        let text = node_map
            .get(node_id)
            .map(|n| escape_html_csv(n.texto.trim()))
            .unwrap_or_default();
        let outgoing = edges_from.get(node_id).map(Vec::as_slice).unwrap_or(&[]);

        let _ = write!(out, "<li>{text}");
        if !outgoing.is_empty() {
            out.push_str("<ul>");
            for edge in outgoing {
                let target_step = step_by_id.get(edge.destino.as_str()).copied().unwrap_or(0);
                let label = edge.rotulo.trim();
                if label.is_empty() {
                    let _ = write!(out, "<li>→ <b>Passo {target_step}</b></li>");
                } else {
                    let _ = write!(
                        out,
                        "<li><b>{}</b> → <b>Passo {target_step}</b></li>",
                        escape_html_csv(label)
                    );
                }
            }
            out.push_str("</ul>");
        }
        out.push_str("</li>");
    }

    out.push_str("</ol></section>");
    out
}

#[must_use]
pub fn render_validated_flow(flow: &ValidatedFlow, titulo: Option<&str>) -> String {
    let node_map: HashMap<&str, &FlowNode> = flow.nos.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut edges_from: HashMap<&str, Vec<&FlowEdge>> = HashMap::with_capacity(flow.nos.len());
    for edge in &flow.arestas {
        edges_from
            .entry(edge.origem.as_str())
            .or_default()
            .push(edge);
    }

    match flow.topology() {
        FlowTopology::Linear => render_linear(flow, titulo, &node_map, &edges_from),
        FlowTopology::ShallowTree => render_shallow_tree(flow, titulo, &node_map, &edges_from),
        FlowTopology::ConvergentDag => render_convergent_dag(flow, titulo, &node_map, &edges_from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, texto: &str) -> FlowNode {
        FlowNode {
            id: id.into(),
            texto: texto.into(),
        }
    }

    fn edge(origem: &str, destino: &str, rotulo: &str) -> FlowEdge {
        FlowEdge {
            origem: origem.into(),
            destino: destino.into(),
            rotulo: rotulo.into(),
        }
    }

    #[test]
    fn rejects_cycle_and_orphan() {
        let cycle = RawFlow {
            inicio: "a".into(),
            nos: vec![node("a", "A"), node("b", "B")],
            arestas: vec![edge("a", "b", ""), edge("b", "a", "")],
        };
        assert_eq!(
            ValidatedFlow::try_from(cycle).unwrap_err(),
            FlowError::CycleDetected("a".into())
        );

        let orphan = RawFlow {
            inicio: "a".into(),
            nos: vec![node("a", "A"), node("b", "B")],
            arestas: vec![],
        };
        assert_eq!(
            ValidatedFlow::try_from(orphan).unwrap_err(),
            FlowError::UnreachableNode("b".into())
        );
    }

    #[test]
    fn rejects_missing_start_and_edge_nodes() {
        let missing_start = RawFlow {
            inicio: "c".into(),
            nos: vec![node("a", "A"), node("b", "B")],
            arestas: vec![edge("a", "b", "")],
        };
        assert_eq!(
            ValidatedFlow::try_from(missing_start).unwrap_err(),
            FlowError::MissingStartNode("c".into())
        );

        let missing_dest = RawFlow {
            inicio: "a".into(),
            nos: vec![node("a", "A")],
            arestas: vec![edge("a", "b", "")],
        };
        assert_eq!(
            ValidatedFlow::try_from(missing_dest).unwrap_err(),
            FlowError::MissingEdgeNode {
                edge_from: "a".into(),
                edge_to: "b".into(),
                missing: "b".into(),
            }
        );
    }

    #[test]
    fn rejects_duplicate_node_id() {
        let dup = RawFlow {
            inicio: "a".into(),
            nos: vec![node("a", "A"), node("a", "A2")],
            arestas: vec![],
        };
        assert_eq!(
            ValidatedFlow::try_from(dup).unwrap_err(),
            FlowError::DuplicateNodeId("a".into())
        );
    }

    #[test]
    fn renders_all_three_topologies_without_br_or_pipes() {
        let linear = ValidatedFlow::try_from(RawFlow {
            inicio: "1".into(),
            nos: vec![node("1", "Estabilizar"), node("2", "Tratar | monitorar")],
            arestas: vec![edge("1", "2", "Depois")],
        })
        .unwrap();
        assert_eq!(linear.topology(), FlowTopology::Linear);
        let html = render_validated_flow(&linear, Some("Conduta"));
        assert!(html.contains("<ol>"));
        assert!(!html.contains('|'));
        assert!(!html.contains("<br"));

        let tree = ValidatedFlow::try_from(RawFlow {
            inicio: "q".into(),
            nos: vec![
                node("q", "Febre?"),
                node("s", "Hemocultura"),
                node("n", "Observar"),
            ],
            arestas: vec![edge("q", "s", "Sim"), edge("q", "n", "Não")],
        })
        .unwrap();
        assert_eq!(tree.topology(), FlowTopology::ShallowTree);
        assert!(render_validated_flow(&tree, None).contains("<b>Sim →</b>"));

        let dag = ValidatedFlow::try_from(RawFlow {
            inicio: "a".into(),
            nos: vec![
                node("a", "Triar"),
                node("b", "Via A"),
                node("c", "Via B"),
                node("d", "Reavaliar"),
            ],
            arestas: vec![
                edge("a", "b", "Estável"),
                edge("a", "c", "Instável"),
                edge("b", "d", ""),
                edge("c", "d", ""),
            ],
        })
        .unwrap();
        assert_eq!(dag.topology(), FlowTopology::ConvergentDag);
        let dag_html = render_validated_flow(&dag, None);
        assert_eq!(dag_html.matches("Reavaliar").count(), 1);
        assert!(dag_html.contains("Passo 4"));
    }
}
