use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const IO_TIMEOUT: Duration = Duration::from_secs(10);

struct McpChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout_rx: Receiver<Result<String, String>>,
    stderr_handle: Option<std::thread::JoinHandle<String>>,
    raw_stdout_lines: Vec<String>,
}

impl McpChild {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn anki-workflow-mcp serve");

        let stdin = child.stdin.take().expect("child stdin piped");
        let stdout = child.stdout.take().expect("child stdout piped");
        let mut stderr = child.stderr.take().expect("child stderr piped");

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                        if tx.send(Ok(trimmed)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(Err(format!("stdout read error: {err}")));
                        break;
                    }
                }
            }
        });

        let stderr_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf);
            buf
        });

        Self {
            child,
            stdin: Some(stdin),
            stdout_rx: rx,
            stderr_handle: Some(stderr_handle),
            raw_stdout_lines: Vec::new(),
        }
    }

    fn send_json(&mut self, msg: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin already closed");
        let encoded = serde_json::to_string(msg).expect("serialize JSON-RPC message");
        writeln!(stdin, "{encoded}").expect("write JSON-RPC line to stdin");
        stdin.flush().expect("flush stdin");
    }

    fn read_json_line(&mut self) -> Value {
        let line = self
            .stdout_rx
            .recv_timeout(IO_TIMEOUT)
            .expect("timed out waiting for JSON-RPC response line")
            .expect("stdout reader failed");
        self.raw_stdout_lines.push(line.clone());
        serde_json::from_str(&line).unwrap_or_else(|err| {
            panic!("stdout emitted non-JSON line ({err}): {line:?}");
        })
    }

    fn request(&mut self, id: i64, method: &str, params: Option<Value>) -> Value {
        let mut req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(p) = params {
            req["params"] = p;
        }
        self.send_json(&req);

        loop {
            let msg = self.read_json_line();
            if msg.get("id") == Some(&json!(id)) {
                return msg;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Option<Value>) {
        let mut note = json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(p) = params {
            note["params"] = p;
        }
        self.send_json(&note);
    }

    fn close_and_wait(mut self) -> (std::process::ExitStatus, String, Vec<String>) {
        drop(self.stdin.take());

        let deadline = Instant::now() + IO_TIMEOUT;
        let status = loop {
            match self.child.try_wait().expect("try_wait failed") {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    let stderr = self
                        .stderr_handle
                        .take()
                        .and_then(|h| h.join().ok())
                        .unwrap_or_default();
                    panic!(
                        "server did not exit cleanly within {:?} after closing stdin; stderr: {}",
                        IO_TIMEOUT, stderr
                    );
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };

        while let Ok(Ok(extra_line)) = self.stdout_rx.try_recv() {
            if !extra_line.is_empty() {
                let _: Value = serde_json::from_str(&extra_line).unwrap_or_else(|err| {
                    panic!("trailing stdout line was not valid JSON ({err}): {extra_line:?}");
                });
                self.raw_stdout_lines.push(extra_line);
            }
        }

        let stderr = self
            .stderr_handle
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        (status, stderr, self.raw_stdout_lines.clone())
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn extract_tool_payload(response: &Value) -> Value {
    let result = response
        .get("result")
        .unwrap_or_else(|| panic!("expected result in response: {response}"));
    if let Some(structured) = result.get("structuredContent") {
        return structured.clone();
    }
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("expected content[0].text in response: {response}"));
    serde_json::from_str(text)
        .unwrap_or_else(|err| panic!("expected JSON text payload ({err}): {text}"))
}

/// Extracts an error payload and enforces the wire contract of `ToolErrorPayload`:
/// `isError: true`, `violations` always present as an array, and `path` either absent
/// or a non-empty string.
fn extract_error_payload(response: &Value) -> Value {
    assert_eq!(
        response["result"]["isError"],
        json!(true),
        "expected isError response: {response}"
    );
    let payload = extract_tool_payload(response);
    assert!(
        payload.get("violations").is_some_and(Value::is_array),
        "ToolErrorPayload.violations must always be emitted as an array: {payload}"
    );
    assert!(
        payload.get("code").is_some_and(Value::is_string),
        "ToolErrorPayload.code must always be emitted: {payload}"
    );
    if let Some(path) = payload.get("path") {
        assert!(
            path.as_str().is_some_and(|p| !p.is_empty()),
            "ToolErrorPayload.path must be a non-empty string when present: {payload}"
        );
    }
    payload
}

fn tool_schema<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    &tools
        .iter()
        .find(|tool| tool["name"] == json!(name))
        .unwrap_or_else(|| panic!("tool {name} must be listed"))["inputSchema"]
}

fn assert_typed_objects_deny_additional_properties(schema: &Value, path: &str) {
    let Some(obj) = schema.as_object() else {
        return;
    };

    if obj.contains_key("properties")
        && !path.ends_with(".properties.draft")
        && !path.ends_with(".properties.updates")
    {
        assert_eq!(
            obj.get("additionalProperties"),
            Some(&Value::Bool(false)),
            "typed object schema at {path} must set additionalProperties: false"
        );
    }

    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        for (key, prop_schema) in props {
            assert_typed_objects_deny_additional_properties(
                prop_schema,
                &format!("{path}.properties.{key}"),
            );
        }
    }

    if let Some(items) = obj.get("items") {
        assert_typed_objects_deny_additional_properties(items, &format!("{path}.items"));
    }

    for combinator in ["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = obj.get(combinator).and_then(Value::as_array) {
            for (idx, sub) in arr.iter().enumerate() {
                assert_typed_objects_deny_additional_properties(
                    sub,
                    &format!("{path}.{combinator}[{idx}]"),
                );
            }
        }
    }

    for defs_key in ["$defs", "definitions"] {
        if let Some(defs) = obj.get(defs_key).and_then(Value::as_object) {
            for (key, def_schema) in defs {
                assert_typed_objects_deny_additional_properties(
                    def_schema,
                    &format!("{path}.{defs_key}.{key}"),
                );
            }
        }
    }
}

#[test]
fn mcp_stdio_end_to_end_contract_tools_errors_and_shutdown() {
    let mut client = McpChild::spawn();

    // 1. Initialize handshake (2025-03-26)
    let init_resp = client.request(
        1,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "mcp-stdio-integration-test",
                "version": "0.1.0"
            }
        })),
    );
    assert!(
        init_resp.get("error").is_none(),
        "initialize returned error: {init_resp}"
    );
    let init_result = &init_resp["result"];
    assert_eq!(init_result["protocolVersion"], json!("2025-03-26"));
    assert_eq!(
        init_result["serverInfo"]["name"],
        json!("anki-workflow-mcp"),
        "serverInfo.name must match package implementation"
    );
    assert_eq!(
        init_result["serverInfo"]["version"],
        json!(env!("CARGO_PKG_VERSION")),
        "serverInfo.version must match CARGO_PKG_VERSION"
    );
    assert!(init_result["capabilities"]["tools"].is_object());
    assert!(
        !init_result["instructions"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .is_empty(),
        "server instructions must be populated"
    );

    // 2. notifications/initialized
    client.notify("notifications/initialized", None);

    // 3. tools/list: exact 7 tools and strict schemas
    let list_resp = client.request(2, "tools/list", None);
    assert!(
        list_resp.get("error").is_none(),
        "tools/list returned error: {list_resp}"
    );
    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("result.tools must be an array");
    assert_eq!(tools.len(), 7, "expected exactly 7 MCP tools");

    let actual_names: BTreeSet<String> = tools
        .iter()
        .map(|t| {
            t["name"]
                .as_str()
                .expect("tool name must be string")
                .to_string()
        })
        .collect();
    let expected_names: BTreeSet<String> = [
        "cards_compile_drafts",
        "review_batch_information",
        "quality_prepare",
        "quality_evaluate",
        "batch_patch_draft",
        "generation_contract_get",
        "export_reviewed_files",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(actual_names, expected_names);

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let schema = &tool["inputSchema"];
        assert_eq!(
            schema.get("type"),
            Some(&json!("object")),
            "tool {name} inputSchema must be object"
        );
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&json!(false)),
            "tool {name} top-level inputSchema must set additionalProperties: false"
        );
        assert_typed_objects_deny_additional_properties(schema, name);

        let ann = &tool["annotations"];
        let expected_read_only = name != "export_reviewed_files";
        assert_eq!(
            ann.get("readOnlyHint"),
            Some(&json!(expected_read_only)),
            "{name} readOnlyHint"
        );
        assert_eq!(
            ann.get("destructiveHint"),
            Some(&json!(false)),
            "{name} destructiveHint"
        );

        if name == "batch_patch_draft" {
            let desc = tool["description"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase();
            assert!(
                desc.contains("in-memory") && desc.contains("python"),
                "batch_patch_draft description must document in-memory vs Python staging difference"
            );
        }

        if name == "quality_prepare" || name == "quality_evaluate" {
            let props = &schema["properties"];
            for nested_path in [
                &props["sources"]["items"],
                &props["obligations"]["items"],
                &props["bindings"]["items"],
                &props["bindings"]["items"]["properties"]["evidence"]["items"],
            ] {
                assert_eq!(
                    nested_path.get("additionalProperties"),
                    Some(&json!(false)),
                    "quality nested object in {name} must set additionalProperties: false: {nested_path}"
                );
            }
        }

        if name == "quality_evaluate" {
            let props = &schema["properties"];
            for nested_path in [
                &props["reviews"]["items"],
                &props["reviews"]["items"]["properties"]["dimensions"],
            ] {
                assert_eq!(
                    nested_path.get("additionalProperties"),
                    Some(&json!(false)),
                    "quality nested object in {name} must set additionalProperties: false: {nested_path}"
                );
            }
        }
    }

    // 3b. B22: campos que decidem comportamento expõem `description` no inputSchema.
    fn description_of<'a>(schema: &'a Value, pointer: &[&str]) -> &'a str {
        let mut node = schema;
        for key in pointer {
            node = &node[*key];
        }
        node.get("description")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing description at {}", pointer.join(".")))
    }

    let compile_schema = tool_schema(tools, "cards_compile_drafts");
    for prop in [
        "auto_renumber_cloze",
        "auto_purge_id_leaks",
        "default_campo_estudo",
    ] {
        assert!(
            !description_of(compile_schema, &["properties", prop]).is_empty(),
            "cards_compile_drafts.{prop} must document its behavior"
        );
    }
    // `compile_mode` / `mode` são `$ref` no contrato exportado: uma `description` ao lado do
    // `$ref` duplicaria o tipo no .d.ts (`CompileMode1`/`QualityMode1`), então a doc desses
    // campos vive na descrição da tool. A guarda do contrato exportado está em `mcp.rs`.
    let compile_desc = tools
        .iter()
        .find(|tool| tool["name"] == json!("cards_compile_drafts"))
        .and_then(|tool| tool["description"].as_str())
        .unwrap_or_default();
    for expected in [
        "auto_renumber_cloze",
        "auto_purge_id_leaks",
        "default_campo_estudo",
        "compile_mode",
        "legacy_compat",
        "strict_v2",
        "COMPILE.INVALID_DRAFT",
    ] {
        assert!(
            compile_desc.contains(expected),
            "cards_compile_drafts description must document {expected}: {compile_desc}"
        );
    }

    for tool_name in ["quality_prepare", "quality_evaluate"] {
        let schema = tool_schema(tools, tool_name);
        let source_row_doc = description_of(
            schema,
            &[
                "properties",
                "bindings",
                "items",
                "properties",
                "source_row",
            ],
        );
        assert!(
            source_row_doc.contains("1-based") && source_row_doc.contains(">= 2"),
            "{tool_name}.bindings[].source_row must explain the 1-based/header rule: {source_row_doc}"
        );
        let cloze_doc = description_of(
            schema,
            &[
                "properties",
                "bindings",
                "items",
                "properties",
                "cloze_index",
            ],
        );
        assert!(
            cloze_doc.contains("CL") && cloze_doc.contains("QA"),
            "{tool_name}.bindings[].cloze_index must explain the CL/QA rule: {cloze_doc}"
        );
        let quote_doc = description_of(
            schema,
            &[
                "properties",
                "bindings",
                "items",
                "properties",
                "evidence",
                "items",
                "properties",
                "quote",
            ],
        );
        assert!(
            quote_doc.contains("verbatim"),
            "{tool_name} evidence.quote must require a verbatim substring: {quote_doc}"
        );
        let desc = tools
            .iter()
            .find(|tool| tool["name"] == json!(tool_name))
            .and_then(|tool| tool["description"].as_str())
            .unwrap_or_default();
        assert!(
            desc.contains("`mode`"),
            "{tool_name} description must document `mode`: {desc}"
        );
    }

    let snapshot_hash_doc = description_of(
        tool_schema(tools, "quality_evaluate"),
        &[
            "properties",
            "reviews",
            "items",
            "properties",
            "snapshot_hash",
        ],
    );
    assert!(
        snapshot_hash_doc.contains("snapshot_hash"),
        "quality_evaluate.reviews[].snapshot_hash must document the snapshot it must echo: {snapshot_hash_doc}"
    );

    // 4. tools/call generation_contract_get
    let contract_resp = client.request(
        3,
        "tools/call",
        Some(json!({
            "name": "generation_contract_get",
            "arguments": {}
        })),
    );
    assert_ne!(contract_resp["result"].get("isError"), Some(&json!(true)));
    let contract = extract_tool_payload(&contract_resp);
    assert_eq!(contract["schema_version"], json!("quality_v1"));
    assert_eq!(contract["no_count_only_limit"], json!(true));
    assert!(contract["phases"].as_array().is_some_and(|a| !a.is_empty()));
    assert!(contract.get("per_retrieval_rubric").is_some());
    assert!(contract.get("field_ordering").is_some());
    assert!(contract.get("limitations").is_some());
    assert!(contract.get("repair_protocol").is_some());

    // 5. tools/call cards_compile_drafts with a small synthetic QA card
    let canary_marker = "SYNTHETIC_QA_MARKER_9911";
    let compile_resp = client.request(
        4,
        "tools/call",
        Some(json!({
            "name": "cards_compile_drafts",
            "arguments": {
                "drafts": [
                    {
                        "tipo": "QA",
                        "enunciado_ou_sentenca": format!("Pergunta sintetica {canary_marker}?"),
                        "resposta": "Resposta sintetica objetiva.",
                        "campo_estudo": "Teste",
                        "referencias": "Manual Sintetico (2026), Secao 1"
                    }
                ]
            }
        })),
    );
    assert_ne!(compile_resp["result"].get("isError"), Some(&json!(true)));
    let compiled = extract_tool_payload(&compile_resp);
    assert_eq!(compiled["stats"]["qa_count"], json!(1));
    assert_eq!(compiled["stats"]["cloze_count"], json!(0));
    let qa_csv = compiled["qa_csv"]
        .as_str()
        .expect("qa_csv string")
        .to_string();
    let cloze_csv = compiled["cloze_csv"]
        .as_str()
        .expect("cloze_csv string")
        .to_string();

    // 6. tools/call review_batch_information returns card counts
    let review_resp = client.request(
        5,
        "tools/call",
        Some(json!({
            "name": "review_batch_information",
            "arguments": {
                "qa_csv": qa_csv,
                "cloze_csv": cloze_csv
            }
        })),
    );
    assert_ne!(review_resp["result"].get("isError"), Some(&json!(true)));
    let review = extract_tool_payload(&review_resp);
    assert_eq!(review["qa_cards"], json!(1));
    assert_eq!(review["cloze_cards"], json!(0));
    assert_eq!(review["total_cards"], json!(1));
    assert_eq!(review["batch_size_limit"], Value::Null);

    // 7. tools/call batch_patch_draft pure in-memory
    let patch_resp = client.request(
        6,
        "tools/call",
        Some(json!({
            "name": "batch_patch_draft",
            "arguments": {
                "draft": {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Pergunta sintetica?",
                    "resposta": "Original."
                },
                "updates": {
                    "resposta": "Atualizada."
                }
            }
        })),
    );
    assert_ne!(patch_resp["result"].get("isError"), Some(&json!(true)));
    let patched = extract_tool_payload(&patch_resp);
    assert_eq!(patched["card"]["resposta"], json!("Atualizada."));

    // 8. tools/call quality_prepare on empty CSVs => INCOMPLETE and no export authorization
    let prep_resp = client.request(
        7,
        "tools/call",
        Some(json!({
            "name": "quality_prepare",
            "arguments": {
                "qa_csv": "",
                "cloze_csv": ""
            }
        })),
    );
    assert_ne!(prep_resp["result"].get("isError"), Some(&json!(true)));
    let prep = extract_tool_payload(&prep_resp);
    let prep_serialized = serde_json::to_string(&prep).expect("serialize quality_prepare result");
    assert!(
        prep_serialized.contains("INCOMPLETE"),
        "empty quality_prepare must return INCOMPLETE, got: {prep_serialized}"
    );
    for auth_key in [
        "export_authorized",
        "export_allowed",
        "can_export",
        "authorized_for_export",
    ] {
        if let Some(flag) = prep.get(auth_key) {
            assert_eq!(
                flag,
                &json!(false),
                "empty quality_prepare must not authorize export ({auth_key})"
            );
        }
    }
    if let Some(receipt) = prep.get("receipt") {
        assert!(
            receipt.is_null(),
            "empty quality_prepare must not emit a receipt"
        );
    }

    // 8b. tools/call quality_prepare + quality_evaluate full synthetic pass lifecycle
    let valid_prep_resp = client.request(
        70,
        "tools/call",
        Some(json!({
            "name": "quality_prepare",
            "arguments": {
                "qa_csv": qa_csv,
                "cloze_csv": cloze_csv,
                "sources": [{
                    "id": "S1",
                    "title": "Manual Sintetico",
                    "version": "1.0",
                    "locator": "Secao 1",
                    "text": format!("Pergunta sintetica {canary_marker}? Resposta sintetica objetiva.")
                }],
                "obligations": [{
                    "id": "O1",
                    "description": "Identificar resposta sintetica.",
                    "source_ids": ["S1"]
                }],
                "bindings": [{
                    "note_type": "QA",
                    "source_row": 2,
                    "obligation_ids": ["O1"],
                    "evidence": [{
                        "source_id": "S1",
                        "quote": "Resposta sintetica objetiva.",
                        "locator": "Secao 1",
                        "scope": "retrieval"
                    }]
                }]
            }
        })),
    );
    assert_ne!(valid_prep_resp["result"].get("isError"), Some(&json!(true)));
    let valid_prep = extract_tool_payload(&valid_prep_resp);
    assert_eq!(valid_prep["verdict"], json!("INCOMPLETE"));
    assert_eq!(valid_prep["export_authorized"], json!(false));
    assert_eq!(valid_prep["clinical_correctness_certified"], json!(false));
    let snapshot_hash = valid_prep["snapshot_hash"]
        .as_str()
        .expect("snapshot_hash string")
        .to_string();

    let eval_pass_resp = client.request(
        71,
        "tools/call",
        Some(json!({
            "name": "quality_evaluate",
            "arguments": {
                "qa_csv": qa_csv,
                "cloze_csv": cloze_csv,
                "sources": [{
                    "id": "S1",
                    "title": "Manual Sintetico",
                    "version": "1.0",
                    "locator": "Secao 1",
                    "text": format!("Pergunta sintetica {canary_marker}? Resposta sintetica objetiva.")
                }],
                "obligations": [{
                    "id": "O1",
                    "description": "Identificar resposta sintetica.",
                    "source_ids": ["S1"]
                }],
                "bindings": [{
                    "note_type": "QA",
                    "source_row": 2,
                    "obligation_ids": ["O1"],
                    "evidence": [{
                        "source_id": "S1",
                        "quote": "Resposta sintetica objetiva.",
                        "locator": "Secao 1",
                        "scope": "retrieval"
                    }]
                }],
                "reviews": [{
                    "note_type": "QA",
                    "source_row": 2,
                    "snapshot_hash": snapshot_hash,
                    "reviewer": "curador-sintetico",
                    "verdict": "approve",
                    "rationale": "Fiel ao excerto sintético.",
                    "dimensions": {
                        "source_fidelity": true,
                        "atomic_retrieval": true,
                        "context_sufficient": true,
                        "no_answer_leak": true,
                        "clinically_relevant": true,
                        "notes_appropriate": true,
                        "media_appropriate": true
                    }
                }]
            }
        })),
    );
    assert_ne!(eval_pass_resp["result"].get("isError"), Some(&json!(true)));
    let eval_pass = extract_tool_payload(&eval_pass_resp);
    assert_eq!(eval_pass["verdict"], json!("PASS"));
    assert_eq!(eval_pass["reviewed_count"], json!(1));
    assert_eq!(eval_pass["covered_obligation_count"], json!(1));
    assert_eq!(eval_pass["export_authorized"], json!(false));
    assert_eq!(eval_pass["clinical_correctness_certified"], json!(false));
    let receipt_hash = anki_workflow_mcp::export::compute_receipt_hash(&qa_csv, &cloze_csv);

    // 8b. export_reviewed_files over stdio (positive export + idempotent replay + negative non-PASS)
    let export_tmp = tempfile::tempdir().expect("tempdir for export_reviewed_files");
    let export_dir_str = export_tmp.path().to_string_lossy().to_string();
    let export_ok_resp = client.request(
        72,
        "tools/call",
        Some(json!({
            "name": "export_reviewed_files",
            "arguments": {
                "deck_dir": export_dir_str,
                "batch_id": "20261120_stdio_test",
                "topic_slug": "stdio_sintetico",
                "qa_csv": qa_csv,
                "cloze_csv": cloze_csv,
                "receipt_hash": receipt_hash,
                "evaluation_verdict": "PASS"
            }
        })),
    );
    assert_ne!(
        export_ok_resp["result"].get("isError"),
        Some(&json!(true)),
        "export_reviewed_files should succeed: {export_ok_resp}"
    );
    let export_ok = extract_tool_payload(&export_ok_resp);
    assert_eq!(export_ok["status"], json!("EXPORTED"));

    let export_replay_resp = client.request(
        73,
        "tools/call",
        Some(json!({
            "name": "export_reviewed_files",
            "arguments": {
                "deck_dir": export_dir_str,
                "batch_id": "20261120_stdio_test",
                "topic_slug": "stdio_sintetico",
                "qa_csv": qa_csv,
                "cloze_csv": cloze_csv,
                "receipt_hash": receipt_hash,
                "evaluation_verdict": "PASS"
            }
        })),
    );
    assert_ne!(
        export_replay_resp["result"].get("isError"),
        Some(&json!(true))
    );
    let export_replay = extract_tool_payload(&export_replay_resp);
    assert_eq!(export_replay["status"], json!("ALREADY_EXPORTED"));

    let export_fail_resp = client.request(
        74,
        "tools/call",
        Some(json!({
            "name": "export_reviewed_files",
            "arguments": {
                "deck_dir": export_dir_str,
                "batch_id": "20261120_stdio_fail",
                "topic_slug": "stdio_fail",
                "qa_csv": qa_csv,
                "cloze_csv": cloze_csv,
                "receipt_hash": receipt_hash,
                "evaluation_verdict": "FAIL"
            }
        })),
    );
    assert_eq!(export_fail_resp["result"]["isError"], json!(true));
    let export_fail_payload = extract_error_payload(&export_fail_resp);
    assert_eq!(
        export_fail_payload["violations"],
        json!([]),
        "export errors must still carry an empty violations array"
    );

    // 9. Unknown top-level and nested argument rejection => isError: true
    let bad_contract_resp = client.request(
        8,
        "tools/call",
        Some(json!({
            "name": "generation_contract_get",
            "arguments": { "unexpected": true }
        })),
    );
    assert_eq!(bad_contract_resp["result"]["isError"], json!(true));
    let bad_contract_payload = extract_error_payload(&bad_contract_resp);
    assert_eq!(bad_contract_payload["code"], json!("INVALID_ARGUMENTS"));
    assert_eq!(bad_contract_payload["violations"], json!([]));

    // 9a. Erro na RAIZ do objeto: `serde_path_to_error` devolve "." e antes o payload saía com
    // `"path": "."`. O contrato só admite path ausente ou caminho não vazio, então raiz é ruído.
    let root_level_resp = client.request(
        13,
        "tools/call",
        Some(json!({
            "name": "quality_prepare",
            "arguments": {}
        })),
    );
    assert_eq!(root_level_resp["result"]["isError"], json!(true));
    let root_level_payload = extract_error_payload(&root_level_resp);
    assert_eq!(root_level_payload["code"], json!("INVALID_ARGUMENTS"));
    assert!(
        root_level_payload.get("path").is_none(),
        "root-level argument errors must omit path entirely: {root_level_payload}"
    );
    let root_level_message = root_level_payload["error"].as_str().unwrap_or_default();
    assert!(
        !root_level_message.contains("at `.`"),
        "root-level argument errors must not name `.` as a path: {root_level_message}"
    );
    assert!(
        root_level_message.contains("Invalid tool arguments:"),
        "root-level message must stay the path-less form: {root_level_message}"
    );

    let bad_quality_resp = client.request(
        9,
        "tools/call",
        Some(json!({
            "name": "quality_prepare",
            "arguments": {
                "qa_csv": "",
                "cloze_csv": "",
                "sources": [
                    {
                        "id": "s1",
                        "title": "T",
                        "version": "v1",
                        "locator": "p1",
                        "text": "excerpt",
                        "extra_nested": 123
                    }
                ]
            }
        })),
    );
    assert_eq!(bad_quality_resp["result"]["isError"], json!(true));
    let bad_quality_payload = extract_error_payload(&bad_quality_resp);
    assert_eq!(bad_quality_payload["code"], json!("INVALID_ARGUMENTS"));
    assert_eq!(
        bad_quality_payload["path"],
        json!("sources[0].extra_nested"),
        "nested argument errors must expose the failing element path"
    );
    assert!(
        bad_quality_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("at `sources[0].extra_nested`"),
        "message must name the failing path: {bad_quality_payload}"
    );

    // 9b. quality_prepare rejects reviews field
    let prep_with_reviews_resp = client.request(
        90,
        "tools/call",
        Some(json!({
            "name": "quality_prepare",
            "arguments": {
                "qa_csv": "",
                "cloze_csv": "",
                "reviews": []
            }
        })),
    );
    assert_eq!(prep_with_reviews_resp["result"]["isError"], json!(true));
    let prep_with_reviews_payload = extract_error_payload(&prep_with_reviews_resp);
    assert_eq!(
        prep_with_reviews_payload["code"],
        json!("INVALID_ARGUMENTS")
    );
    assert_eq!(prep_with_reviews_payload["path"], json!("reviews"));

    // 9c. quality_evaluate accumulates structured range violations
    let eval_range_violations_resp = client.request(
        91,
        "tools/call",
        Some(json!({
            "name": "quality_evaluate",
            "arguments": {
                "qa_csv": "",
                "cloze_csv": "",
                "bindings": [
                    {
                        "note_type": "QA",
                        "source_row": 1,
                        "obligation_ids": [],
                        "evidence": []
                    },
                    {
                        "note_type": "CL",
                        "source_row": 3,
                        "cloze_index": 0,
                        "obligation_ids": [],
                        "evidence": []
                    }
                ]
            }
        })),
    );
    let eval_violation_payload = extract_error_payload(&eval_range_violations_resp);
    assert_eq!(
        eval_violation_payload["code"],
        json!("INVALID_INPUT_RANGES")
    );
    assert!(
        eval_violation_payload.get("path").is_none(),
        "range violations carry structured paths inside violations[], not a top-level path"
    );
    let vios = eval_violation_payload["violations"]
        .as_array()
        .expect("violations array");
    assert_eq!(vios.len(), 2);
    assert_eq!(vios[0]["code"], json!("INVALID_SOURCE_ROW"));
    assert_eq!(vios[1]["code"], json!("INVALID_CLOZE_INDEX"));

    // 9d. enum inválido em args aninhados expõe o path do elemento
    let nested_enum_resp = client.request(
        92,
        "tools/call",
        Some(json!({
            "name": "quality_evaluate",
            "arguments": {
                "qa_csv": "",
                "cloze_csv": "",
                "reviews": [{
                    "note_type": "QA",
                    "source_row": 2,
                    "snapshot_hash": "h",
                    "reviewer": "r",
                    "verdict": "maybe",
                    "rationale": "ok",
                    "dimensions": {
                        "source_fidelity": true,
                        "atomic_retrieval": true,
                        "context_sufficient": true,
                        "no_answer_leak": true,
                        "clinically_relevant": true,
                        "notes_appropriate": true,
                        "media_appropriate": true
                    }
                }]
            }
        })),
    );
    let nested_enum_payload = extract_error_payload(&nested_enum_resp);
    assert_eq!(nested_enum_payload["code"], json!("INVALID_ARGUMENTS"));
    assert_eq!(nested_enum_payload["path"], json!("reviews[0].verdict"));
    assert!(
        nested_enum_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("maybe")
    );

    // 10. Unknown tool name => MCP invalid params (-32602)
    let unknown_tool_resp = client.request(
        10,
        "tools/call",
        Some(json!({
            "name": "unknown_tool_xyz",
            "arguments": {}
        })),
    );
    assert_eq!(
        unknown_tool_resp["error"]["code"],
        json!(-32602),
        "unknown tool must return JSON-RPC INVALID_PARAMS (-32602): {unknown_tool_resp}"
    );

    // 11. Session remains usable after malformed / unknown calls
    let follow_up_resp = client.request(
        11,
        "tools/call",
        Some(json!({
            "name": "generation_contract_get",
            "arguments": {}
        })),
    );
    assert_ne!(follow_up_resp["result"].get("isError"), Some(&json!(true)));
    let follow_up = extract_tool_payload(&follow_up_resp);
    assert_eq!(follow_up["schema_version"], json!("quality_v1"));

    // 12. Close stdin, verify clean bounded exit, stdout only JSON, no card data leaked to stderr
    let (status, stderr, _stdout_lines) = client.close_and_wait();
    assert!(
        status.success(),
        "server exited with non-zero status {status:?}; stderr: {stderr}"
    );
    assert!(
        !stderr.contains(canary_marker),
        "stderr must never log card/source values"
    );
}

fn resolve_property_schema<'a>(schema: &'a Value, name: &str) -> &'a Value {
    let property = &schema["properties"][name];
    let Some(def_name) = property
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|pointer| pointer.strip_prefix("#/$defs/"))
    else {
        return property;
    };
    schema
        .get("$defs")
        .and_then(|defs| defs.get(def_name))
        .unwrap_or(property)
}

fn compile_drafts_call(id: i64, args: Value, client: &mut McpChild) -> Value {
    client.request(
        id,
        "tools/call",
        Some(json!({"name": "cards_compile_drafts", "arguments": args})),
    )
}

#[test]
fn mcp_stdio_compile_mode_contract_and_gating() {
    let mut client = McpChild::spawn();

    let init_resp = client.request(
        1,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "mcp-stdio-compile-mode-test", "version": "0.1.0"}
        })),
    );
    assert!(
        init_resp.get("error").is_none(),
        "initialize returned error: {init_resp}"
    );
    client.notify("notifications/initialized", None);

    // 1. tools/list expõe compile_mode com enum fechado e default fail-safe.
    let list_resp = client.request(2, "tools/list", None);
    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("result.tools must be an array");
    let compile_tool = tools
        .iter()
        .find(|tool| tool["name"] == json!("cards_compile_drafts"))
        .expect("cards_compile_drafts must be listed");
    let schema = &compile_tool["inputSchema"];
    let mode_schema = resolve_property_schema(schema, "compile_mode");
    let mut variants: Vec<String> = mode_schema["enum"]
        .as_array()
        .expect("compile_mode must expose a closed enum")
        .iter()
        .map(|variant| variant.as_str().expect("enum variant string").to_string())
        .collect();
    variants.sort();
    assert_eq!(variants, vec!["legacy_compat", "strict_v2"]);
    assert_eq!(
        mode_schema.get("default"),
        Some(&json!("legacy_compat")),
        "compile_mode default must be legacy_compat: {mode_schema}"
    );

    let legacy_notes_draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta sintetica do gate de modo?",
        "resposta": "Resposta sintetica.",
        "notas_adicionais": {"fisiopatologia": "Mecanismo sintetico legado."}
    });

    // 2. Sem compile_mode: comportamento atual (legacy_compat) já que o lote é válido.
    let default_resp = compile_drafts_call(
        3,
        json!({"drafts": [legacy_notes_draft.clone()]}),
        &mut client,
    );
    assert_ne!(
        default_resp["result"].get("isError"),
        Some(&json!(true)),
        "omitting compile_mode must keep current behavior: {default_resp}"
    );
    let default_payload = extract_tool_payload(&default_resp);
    assert_eq!(default_payload["stats"]["qa_count"], json!(1));
    assert!(
        default_payload["qa_csv"]
            .as_str()
            .unwrap_or_default()
            .contains("Mecanismo sintetico legado."),
        "legacy_compat must render legacy notes"
    );

    // 3. compile_mode: strict_v2 + campo legado => isError com COMPILE.INVALID_DRAFT.
    let strict_legacy_resp = compile_drafts_call(
        4,
        json!({"drafts": [legacy_notes_draft], "compile_mode": "strict_v2"}),
        &mut client,
    );
    assert_eq!(
        strict_legacy_resp["result"]["isError"],
        json!(true),
        "strict_v2 must reject legacy notes fields: {strict_legacy_resp}"
    );
    let strict_legacy_payload = extract_error_payload(&strict_legacy_resp);
    assert_eq!(
        strict_legacy_payload["code"],
        json!("COMPILE.INVALID_DRAFT"),
        "draft compile failures must not be reported as INVALID_ARGUMENTS"
    );
    assert_eq!(strict_legacy_payload["violations"], json!([]));
    let strict_legacy_error = strict_legacy_payload["error"]
        .as_str()
        .expect("error string")
        .to_string();
    assert!(
        strict_legacy_error.contains("drafts[0]: campo legado `fisiopatologia`"),
        "unexpected strict_v2 error: {strict_legacy_error}"
    );

    // 4. compile_mode: strict_v2 + NoteDocument canônico => aceito e renderizado.
    let canonical_resp = compile_drafts_call(
        5,
        json!({
            "drafts": [{
                "tipo": "QA",
                "enunciado_ou_sentenca": "Pergunta sintetica canonica?",
                "resposta": "Resposta sintetica.",
                "notas_adicionais": {
                    "blocos": [{"tipo": "paragrafo", "titulo": "Contexto", "texto": "Texto canonico sintetico."}],
                    "fontes": [{"titulo": "Fonte sintetica"}]
                }
            }],
            "compile_mode": "strict_v2"
        }),
        &mut client,
    );
    assert_ne!(
        canonical_resp["result"].get("isError"),
        Some(&json!(true)),
        "strict_v2 must accept canonical NoteDocument: {canonical_resp}"
    );
    let canonical_payload = extract_tool_payload(&canonical_resp);
    let canonical_csv = canonical_payload["qa_csv"]
        .as_str()
        .expect("qa_csv string")
        .to_string();
    assert!(canonical_csv.contains("anki-note-section"));
    assert!(canonical_csv.contains("Texto canonico sintetico."));

    // 5. compile_mode inválido => INVALID_ARGUMENTS com path do enum (deny_unknown_fields/enum fechado).
    let invalid_mode_resp = compile_drafts_call(
        6,
        json!({
            "drafts": [{"tipo": "QA", "enunciado_ou_sentenca": "P?", "resposta": "R."}],
            "compile_mode": "strict_v3"
        }),
        &mut client,
    );
    let invalid_mode_payload = extract_error_payload(&invalid_mode_resp);
    assert_eq!(invalid_mode_payload["code"], json!("INVALID_ARGUMENTS"));
    assert_eq!(
        invalid_mode_payload["path"],
        json!("compile_mode"),
        "enum errors must expose the failing argument: {invalid_mode_payload}"
    );
    assert!(
        invalid_mode_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Invalid tool arguments at `compile_mode`"),
        "unexpected invalid-mode error: {invalid_mode_payload}"
    );
    assert!(
        invalid_mode_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("strict_v3"),
        "error must quote the rejected value: {invalid_mode_payload}"
    );

    // 6. Sessão segue utilizável e encerra limpo.
    let follow_up_resp = client.request(
        7,
        "tools/call",
        Some(json!({"name": "generation_contract_get", "arguments": {}})),
    );
    assert_ne!(follow_up_resp["result"].get("isError"), Some(&json!(true)));

    let (status, stderr, _stdout_lines) = client.close_and_wait();
    assert!(
        status.success(),
        "server exited with non-zero status {status:?}; stderr: {stderr}"
    );
}
