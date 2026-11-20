/* eslint-disable */
/**
 * AUTO-GENERATED FILE — DO NOT EDIT BY HAND.
 * Source: contracts/v2/schemas/*.schema.json (exported from Rust via schemars).
 * Regenerate with: `just codegen-contracts`.
 */

export type Severity = "CRITICAL" | "WARN" | "INFO" | "LOG";
export type CompatibilityProfile = "legacy_compat" | "strict_v2";
export type StringOrVec = string | string[];
export type OneOrManyFigures = RawMediaFigure | RawMediaFigure[];
export type RawExtraNotesInput = string | NoteDocument | RawExtraNotes;
/**
 * Bloco tipado de conteúdo que compõe um `NoteDocument`.
 * Estados inválidos tornam-se irrepresentáveis em tempo de desserialização.
 */
export type NoteBlock =
  | {
      colapsavel?: string | null;
      texto: string;
      tipo: "paragrafo";
      titulo?: string | null;
    }
  | {
      colapsavel?: string | null;
      pares: KeyValueItem[];
      tipo: "chave_valor";
      titulo?: string | null;
    }
  | {
      rotulo?: string | null;
      texto: string;
      tipo: "callout";
      variante: CalloutKind;
    }
  | {
      colapsavel?: string | null;
      itens: string[];
      ordenada?: boolean;
      tipo: "lista";
      titulo?: string | null;
    }
  | {
      colapsavel?: string | null;
      spec: TableSpec;
      tipo: "tabela";
    }
  | {
      colapsavel?: string | null;
      fluxo: RawFlow;
      tipo: "fluxograma";
      titulo?: string | null;
    }
  | {
      colapsavel?: string | null;
      figura: MediaFigure;
      tipo: "figura";
    };
/**
 * Variante semântica de um bloco de destaque (`Callout`).
 */
export type CalloutKind = "trap" | "pearl" | "diff";
/**
 * Localização estruturada dentro de uma fonte bibliográfica ou de mídia.
 */
export type SourceLocation =
  | {
      formato: "pdf";
      pagina: number;
    }
  | {
      formato: "secao";
      referencia: string;
    }
  | {
      formato: "url";
      href: string;
    }
  | {
      formato: "timestamp";
      marca: string;
    };
export type NoteBlockKind = "paragrafo" | "lista" | "alerta" | "tabela" | "fluxograma";
export type CardTipo = "QA" | "CLOZE";
/**
 * Modo de compilação do documento de notas adicionais.
 */
export type CompileMode = "legacy_compat" | "strict_v2";
export type ContentDigest = string;
export type EvidenceScope = "retrieval" | "context" | "notes";
/**
 * Status of the export operation.
 */
export type ExportStatus = "EXPORTED" | "ALREADY_EXPORTED";
export type NoteType = "QA" | "CL" | "CLOZE";
export type StrictNoteTypeArg = "QA" | "CL";
/**
 * Validation mode for quality checks: `Quick` for lightweight structural/HTML checks,
 * or `Strict` for full source grounding and editorial review checks.
 */
export type QualityMode = "quick" | "strict";
export type ReviewVerdict = "approve" | "revise";
export type ValidationResult = "PASS" | "FAIL" | "INCOMPLETE";

/**
 * Aggregate index of all v2 JSON Schema contracts exported by anki-workflow-mcp.
 */
export interface AnkiWorkflowContractsV2 {
  BatchCount: BatchCount;
  BatchInformationReview: BatchInformationReview;
  BatchPatchDraftArgs: BatchPatchDraftArgs;
  CompatibilityProfile: CompatibilityProfile;
  CompileDraftsArgs: CompileDraftsArgs;
  CompileMode: CompileMode;
  ContentDigest: ContentDigest;
  Diagnostic: Diagnostic;
  DomainProfile: DomainProfile;
  EmptyArgs: EmptyArgs;
  EvidenceScope: EvidenceScope;
  ExportReviewedFilesArgs: ExportReviewedFilesArgs;
  ExportReviewedFilesResult: ExportReviewedFilesResult;
  NoteDocument: NoteDocument;
  NoteType: NoteType;
  QualityEvaluateArgs: QualityEvaluateArgs;
  QualityInput: QualityInput;
  QualityMode: QualityMode;
  QualityPrepareArgs: QualityPrepareArgs;
  QualityPrepareInput: QualityPrepareInput;
  QualityReport: QualityReport;
  ReviewBatchInformationArgs: ReviewBatchInformationArgs;
  ReviewVerdict: ReviewVerdict;
  Severity: Severity;
  ToolErrorPayload: ToolErrorPayload;
  ValidationResult: ValidationResult;
}
export interface BatchCount {
  cloze_indices: number;
  cloze_notes: number;
  excess: number;
  ext_batch_suggested?: string | null;
  qa_cards: number;
  total: number;
  within_limit: boolean;
}
export interface BatchInformationReview {
  cards: CardInformationReview[];
  cloze_cards: number;
  diagnostics: Diagnostic[];
  qa_cards: number;
  semantic_review_required: boolean;
  total_cards: number;
}
export interface CardInformationReview {
  candidate_item_count: number;
  candidate_items: string[];
  cloze_index?: number | null;
  diagnostics: Diagnostic[];
  hidden_list_signal: boolean;
  note_type: string;
  notes_visible_chars: number;
  prompt_visible_chars: number;
  retrieved_target: string;
  retrieved_visible_chars: number;
  semantic_review_required: boolean;
  source_row: number;
  visible_context?: string | null;
  visible_context_chars?: number | null;
}
export interface Diagnostic {
  auto_fix_available: boolean;
  code: string;
  field?: string | null;
  fix_preview?: string | null;
  message: string;
  owner_skill: string;
  row?: number | null;
  severity: Severity;
  suggestion?: string | null;
}
export interface BatchPatchDraftArgs {
  draft: {
    [k: string]: unknown | undefined;
  };
  updates: {
    [k: string]: unknown | undefined;
  };
}
export interface CompileDraftsArgs {
  auto_purge_id_leaks?: boolean;
  auto_renumber_cloze?: boolean;
  default_campo_estudo?: string;
  /**
   * List of card draft objects (QA or Cloze) to normalize and compile.
   */
  drafts: StrictRawCardDraft[];
}
export interface StrictRawCardDraft {
  campo_estudo?: string;
  enunciado_ou_sentenca: string;
  etiquetas?: StringOrVec;
  figura?: RawMediaFigure | null;
  figuras?: RawMediaFigure[];
  figuras_notas?: OneOrManyFigures | null;
  id?: string | null;
  notas_adicionais?: RawExtraNotesInput;
  referencias?: StringOrVec;
  resposta?: string | null;
  tabela_notas?: RawTableSpec | null;
  tabelas_notas?: RawTableSpec[] | null;
  tipo: CardTipo;
}
export interface RawMediaFigure {
  caption?: string;
  caption_style?: string;
  context_link?: string;
  filename: string;
  item_type?: string;
  style?: string;
  title?: string;
}
/**
 * Documento estruturado de nota composto por blocos tipados e fontes deduplicáveis.
 */
export interface NoteDocument {
  blocos?: NoteBlock[];
  fontes?: NoteSource[];
}
/**
 * Par chave/valor exibido em blocos `NoteBlock::ChaveValor`.
 */
export interface KeyValueItem {
  chave: string;
  valor: string;
}
export interface TableSpec {
  caption?: string;
  headers: string[];
  rows: string[][];
}
export interface RawFlow {
  arestas?: FlowEdge[];
  inicio: string;
  nos: FlowNode[];
}
export interface FlowEdge {
  destino: string;
  origem: string;
  rotulo: string;
}
export interface FlowNode {
  id: string;
  texto: string;
}
export interface MediaFigure {
  caption?: string;
  caption_style?: string;
  context_link?: string;
  filename: string;
  item_type?: string;
  style?: string;
  title?: string;
}
/**
 * Fonte bibliográfica ou de referência associada ao documento de nota.
 */
export interface NoteSource {
  localizacao?: SourceLocation | null;
  titulo: string;
  trecho?: string | null;
}
export interface RawExtraNotes {
  armadilhas_diferenciais?: string | null;
  blocos?: RawNoteBlock[];
  correlacao_visual?: string | null;
  fisiopatologia?: string | null;
  raciocinio_clinico?: string | null;
  tabela?: RawTableSpec | null;
  texto_livre?: string | null;
}
export interface RawNoteBlock {
  fluxograma?: RawNoteFlow | null;
  fontes: RawNoteSource[];
  itens?: string[];
  tabela?: RawTableSpec | null;
  texto?: string;
  tipo: NoteBlockKind;
  titulo?: string;
}
export interface RawNoteFlow {
  arestas?: RawFlowEdge[];
  inicio: string;
  nos: RawFlowNode[];
}
export interface RawFlowEdge {
  destino: string;
  origem: string;
  rotulo: string;
}
export interface RawFlowNode {
  id: string;
  texto: string;
}
export interface RawNoteSource {
  fonte: string;
  pagina: number;
  trecho: string;
}
export interface RawTableSpec {
  caption?: string;
  headers: string[];
  rows: string[][];
}
export interface DomainProfile {
  default_max_note_chars: number;
  description: string;
  editorial_checklist?: EditorialChecklistItem[];
  exam_boilerplate: ExamBoilerplateConfig;
  id: string;
  info_policy: InfoPolicy;
  language: string;
  name: string;
  table_max_note_chars: number;
}
export interface EditorialChecklistItem {
  description: string;
  id: string;
  label: string;
}
export interface ExamBoilerplateConfig {
  enabled?: boolean;
  known_labels?: string[];
  phrase_patterns?: string[];
}
export interface InfoPolicy {
  forbid_exam_only_mentions?: boolean;
  forbid_metadata_headers?: boolean;
  forbid_verbatim_answer_restatement?: boolean;
  notes_purpose: string;
  preferred_list_format: string;
  source_fidelity: string;
}
export interface EmptyArgs {}
/**
 * Arguments for `export_reviewed_files`.
 */
export interface ExportReviewedFilesArgs {
  batch_id: string;
  cloze_csv: string;
  deck_dir: string;
  evaluation_verdict: string;
  qa_csv: string;
  receipt_hash: string;
  topic_slug?: string | null;
  write_to_deck_root?: boolean;
}
/**
 * Result of `export_reviewed_files`.
 */
export interface ExportReviewedFilesResult {
  batch_id: string;
  cloze_file: string;
  qa_file: string;
  receipt_hash: string;
  root_mirror_written: boolean;
  state_file: string;
  status: ExportStatus;
}
export interface QualityEvaluateArgs {
  bindings?: QualityBindingArg[];
  cloze_csv: string;
  mode?: QualityMode;
  obligations?: QualityObligationArg[];
  qa_csv: string;
  reviews?: QualityReviewArg[];
  sources?: QualitySourceArg[];
}
export interface QualityBindingArg {
  cloze_index?: number | null;
  evidence: QualityEvidenceArg[];
  note_type: StrictNoteTypeArg;
  obligation_ids: string[];
  source_row: number;
}
export interface QualityEvidenceArg {
  locator: string;
  quote: string;
  scope: EvidenceScope;
  source_id: string;
}
export interface QualityObligationArg {
  description: string;
  id: string;
  source_ids: string[];
}
export interface QualityReviewArg {
  cloze_index?: number | null;
  dimensions: ReviewDimensionsArg;
  note_type: StrictNoteTypeArg;
  rationale: string;
  reviewer: string;
  snapshot_hash: string;
  source_row: number;
  verdict: ReviewVerdict;
}
export interface ReviewDimensionsArg {
  atomic_retrieval: boolean;
  clinically_relevant: boolean;
  context_sufficient: boolean;
  media_appropriate: boolean;
  no_answer_leak: boolean;
  notes_appropriate: boolean;
  source_fidelity: boolean;
}
export interface QualitySourceArg {
  id: string;
  locator: string;
  text: string;
  title: string;
  version: string;
}
export interface QualityInput {
  bindings?: QualityBinding[];
  cloze_csv: string;
  mode?: QualityMode;
  obligations?: QualityObligation[];
  qa_csv: string;
  reviews?: QualityReview[];
  sources?: QualitySource[];
}
export interface QualityBinding {
  cloze_index?: number | null;
  evidence: QualityEvidence[];
  note_type: string;
  obligation_ids: string[];
  source_row: number;
}
export interface QualityEvidence {
  locator: string;
  quote: string;
  scope: string;
  source_id: string;
}
export interface QualityObligation {
  description: string;
  id: string;
  source_ids: string[];
}
export interface QualityReview {
  cloze_index?: number | null;
  dimensions: QualityDimensions;
  note_type: string;
  rationale: string;
  reviewer: string;
  snapshot_hash: string;
  source_row: number;
  verdict: string;
}
export interface QualityDimensions {
  atomic_retrieval: boolean;
  clinically_relevant: boolean;
  context_sufficient: boolean;
  media_appropriate: boolean;
  no_answer_leak: boolean;
  notes_appropriate: boolean;
  source_fidelity: boolean;
}
export interface QualitySource {
  id: string;
  locator: string;
  text: string;
  title: string;
  version: string;
}
export interface QualityPrepareArgs {
  bindings?: QualityBindingArg[];
  cloze_csv: string;
  mode?: QualityMode;
  obligations?: QualityObligationArg[];
  qa_csv: string;
  sources?: QualitySourceArg[];
}
export interface QualityPrepareInput {
  bindings?: QualityBinding[];
  cloze_csv: string;
  mode?: QualityMode;
  obligations?: QualityObligation[];
  qa_csv: string;
  sources?: QualitySource[];
}
export interface QualityReport {
  clinical_correctness_certified: boolean;
  content_hash: string;
  covered_obligation_count: number;
  diagnostics: Diagnostic[];
  export_authorized: boolean;
  guidance: string[];
  mode: string;
  obligation_count: number;
  retrieval_count: number;
  review: BatchInformationReview;
  reviewed_count: number;
  schema_version: string;
  snapshot_hash: string;
  verdict: string;
}
export interface ReviewBatchInformationArgs {
  cloze_csv: string;
  qa_csv: string;
}
export interface ToolErrorPayload {
  code: string;
  error: string;
  violations: InputViolation[];
}
export interface InputViolation {
  code: string;
  message: string;
  path: string;
}
