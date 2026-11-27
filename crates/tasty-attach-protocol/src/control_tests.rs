//! Control 메시지·상수의 wire 형식 시험.

use super::*;

#[test]
fn a_descriptor_without_pty_input_reads_as_vt() {
    // 구 서버는 키를 보내지 않는다. Windows 서버를 VT로 오판하는 쪽만 남고 Unix 서버는 맞는다.
    assert!(!descriptor_wants_win32_console_input(
        &serde_json::json!({})
    ));
    assert!(!descriptor_wants_win32_console_input(
        &serde_json::json!({ DESCRIPTOR_PTY_INPUT: "future" })
    ));
    assert!(!descriptor_wants_win32_console_input(
        &serde_json::json!({ DESCRIPTOR_PTY_INPUT: PTY_INPUT_VT })
    ));
    assert!(descriptor_wants_win32_console_input(
        &serde_json::json!({ DESCRIPTOR_PTY_INPUT: PTY_INPUT_WIN32_CONSOLE })
    ));
}

#[test]
fn stream_control_bulk_begin_commit_result_roundtrip() {
    let begin = StreamControl::BulkBegin {
        transfer_id: 9,
        filename: "img.png".to_string(),
        total_size: 123_456,
    };
    let s = serde_json::to_string(&begin).unwrap();
    assert!(s.contains(r#""event":"bulk_begin""#));
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), begin);

    let commit = StreamControl::BulkCommit { transfer_id: 9 };
    let s = serde_json::to_string(&commit).unwrap();
    assert!(s.contains(r#""event":"bulk_commit""#));
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), commit);

    let ok = StreamControl::BulkResult {
        transfer_id: 9,
        ok: true,
        path: Some("/home/u/.tasty/transfers/img.png".to_string()),
        reason: None,
    };
    let s = serde_json::to_string(&ok).unwrap();
    assert!(s.contains(r#""event":"bulk_result""#));
    assert!(!s.contains("reason")); // skipped when None
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), ok);

    let fail = StreamControl::BulkResult {
        transfer_id: 9,
        ok: false,
        path: None,
        reason: Some("bound workspace has no active holder".to_string()),
    };
    let s = serde_json::to_string(&fail).unwrap();
    assert!(!s.contains("path")); // skipped when None
    assert!(s.contains("no active holder"));
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), fail);
}

#[test]
fn bulk_events_are_distinct_from_capture_and_structural() {
    // A bulk_begin must not be misread as a foreign event and vice-versa —
    // the `event` tag keeps the mid-session control messages disjoint.
    let begin = serde_json::to_string(&StreamControl::BulkBegin {
        transfer_id: 1,
        filename: "x".to_string(),
        total_size: 0,
    })
    .unwrap();
    assert!(matches!(
        serde_json::from_str::<StreamControl>(&begin).unwrap(),
        StreamControl::BulkBegin { .. }
    ));
    // The screenshot capture-upload events are NOT StreamControl variants — they must
    // still fail to parse as one (bulk added no accidental collision).
    for capture in [
        r#"{"event":"capture_chunk","upload_id":1,"seq":0,"total":1,"data_b64":"AA=="}"#,
        r#"{"event":"capture_commit","upload_id":1,"file_name":"x.png"}"#,
    ] {
        assert!(serde_json::from_str::<StreamControl>(capture).is_err());
    }
}

#[test]
fn stream_control_resize_roundtrip() {
    let msg = StreamControl::Resize {
        surface_id: 7,
        cols: 157,
        rows: 45,
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"resize""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn stream_control_activity_roundtrip() {
    let msg = StreamControl::Activity {
        surface_id: 9,
        busy: true,
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"activity""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn stream_control_foreground_roundtrip_including_cleared() {
    for name in [Some("claude".to_string()), None] {
        let msg = StreamControl::Foreground {
            surface_id: 9,
            name,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"foreground""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }
    let cleared = r#"{"event":"foreground","surface_id":3,"name":null}"#;
    assert_eq!(
        serde_json::from_str::<StreamControl>(cleared).unwrap(),
        StreamControl::Foreground {
            surface_id: 3,
            name: None
        }
    );
}

#[test]
fn stream_control_cwd_roundtrip_including_cleared() {
    for cwd in [Some("/srv/proj".to_string()), None] {
        let msg = StreamControl::Cwd { surface_id: 9, cwd };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"cwd""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }
    let cleared = r#"{"event":"cwd","surface_id":3,"cwd":null}"#;
    assert_eq!(
        serde_json::from_str::<StreamControl>(cleared).unwrap(),
        StreamControl::Cwd {
            surface_id: 3,
            cwd: None
        }
    );
}

#[test]
fn stream_control_client_resize_roundtrip() {
    let msg = StreamControl::ClientResize {
        surface_id: 12,
        cols: 203,
        rows: 57,
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"client_resize""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn stream_control_resize_rejected_roundtrip() {
    let msg = StreamControl::ResizeRejected {
        surface_id: 12,
        cols: 203,
        rows: 57,
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"resize_rejected""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

/// 거절 회신을 모르는 구 client의 enum에서는 역직렬화가 실패해 그 client가 무시한다.
#[test]
fn resize_rejected_fails_to_decode_in_a_client_without_the_variant() {
    #[derive(Debug, serde::Deserialize)]
    #[serde(tag = "event", rename_all = "snake_case")]
    #[allow(dead_code)] // 이유: 구 client의 enum 모양만 흉내 낸다. 필드는 읽지 않는다.
    enum OlderControl {
        Resize {
            surface_id: u32,
            cols: usize,
            rows: usize,
        },
    }
    let s = serde_json::to_string(&StreamControl::ResizeRejected {
        surface_id: 1,
        cols: 80,
        rows: 24,
    })
    .unwrap();
    assert!(serde_json::from_str::<OlderControl>(&s).is_err());
}

#[test]
fn client_resize_and_resize_are_distinct_events() {
    let client = serde_json::to_string(&StreamControl::ClientResize {
        surface_id: 1,
        cols: 80,
        rows: 24,
    })
    .unwrap();
    assert!(matches!(
        serde_json::from_str::<StreamControl>(&client).unwrap(),
        StreamControl::ClientResize { .. }
    ));
    let server = serde_json::to_string(&StreamControl::Resize {
        surface_id: 1,
        cols: 80,
        rows: 24,
    })
    .unwrap();
    assert!(matches!(
        serde_json::from_str::<StreamControl>(&server).unwrap(),
        StreamControl::Resize { .. }
    ));
}

#[test]
fn only_a_lost_workspace_is_not_a_final_force_detach() {
    assert_eq!(
        force_detach_notice(br#"{"event":"force_detached","reason":"force_detach_workspace"}"#),
        Some(ForceDetachNotice::Final)
    );
    assert_eq!(
        force_detach_notice(br#"{"event":"force_detached"}"#),
        Some(ForceDetachNotice::Final)
    );
    assert_eq!(
        force_detach_notice(br#"{"event":"force_detached","reason":"workspace_gone"}"#),
        Some(ForceDetachNotice::WorkspaceGone)
    );
}

#[test]
fn a_control_that_only_mentions_force_detached_is_not_a_notice() {
    // 원격 디렉터리 목록처럼 값에 같은 문자열이 든 정상 Control.
    assert_eq!(
        force_detach_notice(
            br#"{"event":"list_dir_result","entries":[{"name":"force_detached.txt"}]}"#
        ),
        None
    );
    assert_eq!(
        force_detach_notice(br#"{"event":"x","reason":"force_detached"}"#),
        None
    );
    assert_eq!(force_detach_notice(b"force_detached"), None);
}

#[test]
fn stream_control_ignores_foreign_events() {
    // Handshake/other events must NOT deserialize as a StreamControl variant —
    // the client relies on this to skip payloads it doesn't handle.
    for foreign in [
        r#"{"event":"attached","surface_id":1,"cols":80,"rows":24}"#,
        r#"{"event":"attach_error","reason":"x"}"#,
        r#"{"event":"force_detached"}"#,
    ] {
        assert!(serde_json::from_str::<StreamControl>(foreign).is_err());
    }
}

#[test]
fn stream_control_structural_op_roundtrip() {
    let msg = StreamControl::StructuralOp {
        op_id: 42,
        op: StructuralOp::SplitSurface {
            surface_id: 7,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            inherit_cwd: None,
        },
        origin: Some(ForwardOrigin::User),
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"structural_op""#));
    assert!(s.contains(r#""kind":"split_surface""#));
    assert!(s.contains(r#""direction":"horizontal""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn stream_control_structural_op_all_kinds_roundtrip() {
    let ops = [
        StructuralOp::SplitPane {
            anchor_surface_id: 3,
            direction: SplitAxis::Vertical,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({"a": 1}),
            inherit_cwd: None,
        },
        StructuralOp::NewTab {
            anchor_surface_id: 4,
            surface_kind: "markdown".to_string(),
            params: serde_json::json!({"file": "/x"}),
            inherit_cwd: None,
        },
        StructuralOp::CloseSurface { surface_id: 5 },
        StructuralOp::CloseTab {
            anchor_surface_id: 6,
        },
        StructuralOp::ClosePane {
            anchor_surface_id: 7,
        },
        StructuralOp::MoveTab {
            anchor_surface_id: 8,
            from_index: 0,
            to_index: 2,
        },
        StructuralOp::ConvertSurface {
            surface_id: 9,
            surface_kind: "image".to_string(),
            params: serde_json::json!({}),
            cwd: Some("/tmp/proj".to_string()),
            inherit_cwd: None,
        },
        StructuralOp::MoveSurface {
            source_surface_id: 10,
            target_surface_id: 11,
        },
    ];
    for op in ops {
        let msg = StreamControl::StructuralOp {
            op_id: 1,
            op: op.clone(),
            origin: Some(ForwardOrigin::Agent),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg, "roundtrip failed for {op:?}");
    }
}

/// origin을 보내지 않는 client의 close도 User로 읽어 기존 복원 동작을 유지한다.
#[test]
fn a_structural_op_without_origin_is_a_users_op() {
    let raw = r#"{"event":"structural_op","op_id":5,"op":{"kind":"close_surface","surface_id":3}}"#;
    let StreamControl::StructuralOp { op_id, op, origin } =
        serde_json::from_str::<StreamControl>(raw).unwrap()
    else {
        panic!("expected structural_op");
    };
    assert_eq!(
        (op_id, op),
        (5, StructuralOp::CloseSurface { surface_id: 3 })
    );
    assert_eq!(origin, None);
    assert_eq!(ForwardOrigin::of_wire(origin), ForwardOrigin::User);
}

/// 알려진 variant의 새 필드는 구 parser가 무시하되 프레임 전체를 버리지 않는다.
#[test]
fn the_origin_field_is_spelled_and_ignored_as_documented() {
    let msg = StreamControl::StructuralOp {
        op_id: 1,
        op: StructuralOp::CloseTab {
            anchor_surface_id: 2,
        },
        origin: Some(ForwardOrigin::Agent),
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""origin":"agent""#), "{s}");
    let unknown_key =
        r#"{"event":"client_resize","surface_id":1,"cols":80,"rows":24,"origin":"agent"}"#;
    assert!(serde_json::from_str::<StreamControl>(unknown_key).is_ok());
}

/// 모르는 origin은 Agent, 생략/null은 User로 읽는다.
#[test]
fn an_unknown_origin_keeps_the_frame_and_reads_as_agent() {
    let with = |origin: &str| {
        format!(
            r#"{{"event":"structural_op","op_id":9,"op":{{"kind":"close_surface","surface_id":3}},"origin":{origin}}}"#
        )
    };
    let origin_of = |raw: &str| match serde_json::from_str::<StreamControl>(raw) {
        Ok(StreamControl::StructuralOp {
            op_id: 9, origin, ..
        }) => ForwardOrigin::of_wire(origin),
        other => panic!("the frame must parse as the same structural_op: {other:?}"),
    };
    assert_eq!(origin_of(&with(r#""plugin""#)), ForwardOrigin::Agent);
    assert_eq!(origin_of(&with("7")), ForwardOrigin::Agent);
    assert_eq!(origin_of(&with(r#"{"by":"x"}"#)), ForwardOrigin::Agent);
    assert_eq!(origin_of(&with(r#""user""#)), ForwardOrigin::User);
    assert_eq!(origin_of(&with(r#""agent""#)), ForwardOrigin::Agent);
    assert_eq!(origin_of(&with("null")), ForwardOrigin::User);
    let absent =
        r#"{"event":"structural_op","op_id":9,"op":{"kind":"close_surface","surface_id":3}}"#;
    assert_eq!(origin_of(absent), ForwardOrigin::User);
}

/// `wire_kind` is a second spelling of the serde tag — it must not drift.
#[test]
fn wire_kind_is_the_serde_tag() {
    let ops = [
        StructuralOp::SplitSurface {
            surface_id: 1,
            direction: SplitAxis::Vertical,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            inherit_cwd: None,
        },
        StructuralOp::SplitPane {
            anchor_surface_id: 1,
            direction: SplitAxis::Vertical,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            inherit_cwd: None,
        },
        StructuralOp::NewTab {
            anchor_surface_id: 1,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            inherit_cwd: None,
        },
        StructuralOp::CloseSurface { surface_id: 1 },
        StructuralOp::CloseTab {
            anchor_surface_id: 1,
        },
        StructuralOp::ClosePane {
            anchor_surface_id: 1,
        },
        StructuralOp::MoveTab {
            anchor_surface_id: 1,
            from_index: 0,
            to_index: 1,
        },
        StructuralOp::ConvertSurface {
            surface_id: 1,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            cwd: None,
            inherit_cwd: None,
        },
        StructuralOp::MoveSurface {
            source_surface_id: 1,
            target_surface_id: 2,
        },
        StructuralOp::RestoreClosedItem {
            anchor_surface_id: 1,
        },
        StructuralOp::CloseWorkspace {
            anchor_surface_id: 1,
        },
    ];
    for op in ops {
        let v = serde_json::to_value(&op).unwrap();
        assert_eq!(v["kind"], op.wire_kind(), "{op:?}");
    }
}

#[test]
fn structural_op_defaults_terminal_kind_and_empty_params() {
    // Client may omit surface_kind/params for a plain terminal split.
    let raw = r#"{"kind":"split_surface","surface_id":1,"direction":"vertical"}"#;
    let op: StructuralOp = serde_json::from_str(raw).unwrap();
    match op {
        StructuralOp::SplitSurface {
            surface_kind,
            params,
            ..
        } => {
            assert_eq!(surface_kind, "terminal");
            assert_eq!(params, serde_json::Value::Null);
        }
        _ => panic!("expected split_surface"),
    }
}

/// wire 하위호환: `cwd` 키를 보내지 않는 구버전 client 의 convert op 도
/// 그대로 역직렬화되고 `cwd: None` 이 된다(서버가 자체 resolve 로 폴백).
#[test]
fn convert_surface_without_cwd_key_deserializes_to_none() {
    let raw = r#"{"kind":"convert_surface","surface_id":7,"surface_kind":"explorer","params":{}}"#;
    let op: StructuralOp = serde_json::from_str(raw).unwrap();
    match op {
        StructuralOp::ConvertSurface {
            cwd, surface_id, ..
        } => {
            assert_eq!(surface_id, 7);
            assert!(cwd.is_none());
        }
        other => panic!("expected convert_surface, got {other:?}"),
    }
}

#[test]
fn stream_control_structural_result_roundtrip() {
    let ok = StreamControl::StructuralResult {
        created_surface_id: None,
        op_id: 9,
        ok: true,
        reason: None,
        history_omitted: None,
    };
    let s = serde_json::to_string(&ok).unwrap();
    assert!(s.contains(r#""event":"structural_result""#));
    assert!(!s.contains("reason")); // skipped when None
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), ok);

    let fail = StreamControl::StructuralResult {
        created_surface_id: None,
        op_id: 9,
        ok: false,
        reason: Some("unsupported kind: markdown".to_string()),
        history_omitted: None,
    };
    let s = serde_json::to_string(&fail).unwrap();
    assert!(s.contains("unsupported kind"));
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), fail);

    // An older client ignores the count; an older server never sends it.
    let omitted = StreamControl::StructuralResult {
        created_surface_id: None,
        op_id: 9,
        ok: true,
        reason: None,
        history_omitted: Some(2),
    };
    let s = serde_json::to_string(&omitted).unwrap();
    assert!(s.contains(r#""history_omitted":2"#), "{s}");
    assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), omitted);
    let older: StreamControl =
        serde_json::from_str(r#"{"event":"structural_result","op_id":9,"ok":true}"#).unwrap();
    assert!(matches!(
        older,
        StreamControl::StructuralResult {
            history_omitted: None,
            ..
        }
    ));
}

#[test]
fn stream_control_structural_delta_roundtrip() {
    let msg = StreamControl::StructuralDelta {
        workspace_id: 3,
        tree: serde_json::json!({
            "panes": [{ "id": 1, "tabs": [] }],
            "focused_pane": 1,
        }),
        surfaces: vec![
            serde_json::json!({"remote_id": 10, "role": "terminal", "cols": 80, "rows": 24}),
            serde_json::json!({"remote_id": 11, "role": "placeholder", "kind": "markdown"}),
        ],
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"structural_delta""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn structural_delta_not_confused_with_other_events() {
    let delta = serde_json::to_string(&StreamControl::StructuralDelta {
        workspace_id: 1,
        tree: serde_json::Value::Null,
        surfaces: vec![],
    })
    .unwrap();
    match serde_json::from_str::<StreamControl>(&delta).unwrap() {
        StreamControl::StructuralDelta { workspace_id, .. } => assert_eq!(workspace_id, 1),
        other => panic!("expected StructuralDelta, got {other:?}"),
    }
    assert!(matches!(
        serde_json::from_str::<StreamControl>(
            r#"{"event":"resize","surface_id":1,"cols":80,"rows":24}"#
        )
        .unwrap(),
        StreamControl::Resize { .. }
    ));
}

#[test]
fn structural_op_anchor_surface_id() {
    assert_eq!(
        StructuralOp::CloseSurface { surface_id: 5 }.anchor_surface_id(),
        5
    );
    assert_eq!(
        StructuralOp::ClosePane {
            anchor_surface_id: 9
        }
        .anchor_surface_id(),
        9
    );
    assert_eq!(
        StructuralOp::MoveSurface {
            source_surface_id: 3,
            target_surface_id: 4
        }
        .anchor_surface_id(),
        3
    );
}

#[test]
fn structural_op_with_anchor_surface_id() {
    let local = StructuralOp::SplitPane {
        anchor_surface_id: 5, // local mirror id
        direction: SplitAxis::Vertical,
        surface_kind: "terminal".to_string(),
        params: serde_json::json!({}),
        inherit_cwd: None,
    };
    let remote = local.with_anchor_surface_id(100);
    assert_eq!(remote.anchor_surface_id(), 100);
    let mv = StructuralOp::MoveSurface {
        source_surface_id: 1,
        target_surface_id: 2,
    }
    .with_anchor_surface_id(9);
    match mv {
        StructuralOp::MoveSurface {
            source_surface_id,
            target_surface_id,
        } => {
            assert_eq!(source_surface_id, 9);
            assert_eq!(
                target_surface_id, 2,
                "anchor 치환은 target 을 바꾸지 않는다"
            );
        }
        _ => unreachable!(),
    }
    assert_eq!(mv.move_target_surface_id(), Some(2));
    let mv = mv.with_move_target_surface_id(11);
    assert_eq!(mv.move_target_surface_id(), Some(11));
    assert_eq!(mv.anchor_surface_id(), 9);
    assert_eq!(remote.move_target_surface_id(), None);
    assert_eq!(remote.with_move_target_surface_id(11), remote);
}

#[test]
fn split_axis_ipc_str() {
    assert_eq!(SplitAxis::Vertical.as_ipc_str(), "vertical");
    assert_eq!(SplitAxis::Horizontal.as_ipc_str(), "horizontal");
}

#[test]
fn stream_control_mesh_context_roundtrip() {
    use tasty_plugin_protocol::protocol::ThemeWire;
    use tasty_type_appearance::theme::ThemeColors;

    // 큰 ThemeColors 객체가 json! 재귀 한도를 넘지 않도록 JSON 문자열로 만든다.
    const COLORS_JSON: &str = r##"{
        "crust":"#11111b","mantle":"#181825","base":"#1e1e2e","surface0":"#313244",
        "surface1":"#45475a","surface2":"#585b70","overlay0":"#6c7086","overlay1":"#7f849c",
        "overlay2":"#9399b2","text":"#cdd6f4","subtext1":"#bac2de","subtext0":"#a6adc8",
        "placeholder":"#9399b2","blue":"#89b4fa","green":"#a6e3a1","red":"#f38ba8",
        "yellow":"#f9e2af","peach":"#fab387","mauve":"#cba6f7","teal":"#94e2d5",
        "sky":"#89dceb","lavender":"#b4befe","flamingo":"#f2cdcd","pink":"#f5c2e7",
        "maroon":"#eba0ac","rosewater":"#f5e0dc","selection_bg":"#585b70",
        "vi_cursor_bg":"#f9e2af","search_match_bg":"#f9e2af","search_match_active_bg":"#fab387","search_match_active_fg":"#1e1e2e",
        "ansi_black":"#45475a","ansi_red":"#f38ba8","ansi_green":"#a6e3a1","ansi_yellow":"#f9e2af",
        "ansi_blue":"#89b4fa","ansi_magenta":"#f5c2e7","ansi_cyan":"#94e2d5","ansi_white":"#bac2de",
        "ansi_bright_black":"#585b70","ansi_bright_red":"#f38ba8","ansi_bright_green":"#a6e3a1",
        "ansi_bright_yellow":"#f9e2af","ansi_bright_blue":"#89b4fa","ansi_bright_magenta":"#f5c2e7",
        "ansi_bright_cyan":"#94e2d5","ansi_bright_white":"#a6adc8"
    }"##;
    let colors: ThemeColors = serde_json::from_str(COLORS_JSON).unwrap();

    let msg = StreamControl::MeshContext {
        surface_id: 5,
        width_px: 800,
        height_px: 600,
        pixels_per_point: 2.0,
        theme: Some(ThemeWire {
            colors,
            is_light: false,
            ui_zoom: 1.0,
        }),
        focused: true,
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"mesh_context""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);

    let no_theme = StreamControl::MeshContext {
        surface_id: 6,
        width_px: 100,
        height_px: 100,
        pixels_per_point: 1.0,
        theme: None,
        focused: false,
    };
    let s2 = serde_json::to_string(&no_theme).unwrap();
    assert!(!s2.contains("theme"));
    assert_eq!(
        serde_json::from_str::<StreamControl>(&s2).unwrap(),
        no_theme
    );
}

#[test]
fn stream_control_mesh_input_roundtrip() {
    use tasty_plugin_protocol::protocol::{RawInputEventWire, RawInputWire};

    let msg = StreamControl::MeshInput {
        surface_id: 8,
        input: RawInputWire {
            time: Some(1.5),
            focused: true,
            modifiers: Default::default(),
            events: vec![
                RawInputEventWire::PointerMoved { x: 10.0, y: 20.0 },
                RawInputEventWire::PointerGone,
            ],
        },
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"mesh_input""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn stream_control_mesh_full_resend_request_roundtrip() {
    let msg = StreamControl::MeshFullResendRequest { surface_id: 3 };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"mesh_full_resend_request""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn stream_control_mesh_error_roundtrip() {
    let msg = StreamControl::MeshError {
        surface_id: 11,
        reason: "surface is not egui-mesh whitelisted".to_string(),
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"mesh_error""#));
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn mesh_events_are_distinct_from_each_other_and_existing_events() {
    let ctx = serde_json::to_string(&StreamControl::MeshContext {
        surface_id: 1,
        width_px: 1,
        height_px: 1,
        pixels_per_point: 1.0,
        theme: None,
        focused: false,
    })
    .unwrap();
    assert!(matches!(
        serde_json::from_str::<StreamControl>(&ctx).unwrap(),
        StreamControl::MeshContext { .. }
    ));
    let resend =
        serde_json::to_string(&StreamControl::MeshFullResendRequest { surface_id: 1 }).unwrap();
    assert!(matches!(
        serde_json::from_str::<StreamControl>(&resend).unwrap(),
        StreamControl::MeshFullResendRequest { .. }
    ));
    let resize = serde_json::to_string(&StreamControl::Resize {
        surface_id: 1,
        cols: 80,
        rows: 24,
    })
    .unwrap();
    assert!(matches!(
        serde_json::from_str::<StreamControl>(&resize).unwrap(),
        StreamControl::Resize { .. }
    ));
}

#[test]
fn stream_control_loss_roundtrip() {
    let msg = StreamControl::Loss { frames: 1734 };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"loss""#), "{s}");
    assert!(s.contains(r#""frames":1734"#), "{s}");
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn client_loss_notify_roundtrip() {
    let msg = StreamControl::ClientLossNotify {};
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""event":"client_loss_notify""#), "{s}");
    let back: StreamControl = serde_json::from_str(&s).unwrap();
    assert_eq!(back, msg);
}

/// 모르는 event의 파싱 실패를 확인한다. 구 peer도 Loss를 무시하므로 명시적 선언이 필요하다.
#[test]
fn an_unknown_event_fails_to_parse_so_a_missed_loss_reads_as_no_loss() {
    let from_a_newer_build = r#"{"event":"a_variant_that_does_not_exist_here","frames":9}"#;
    assert!(serde_json::from_str::<StreamControl>(from_a_newer_build).is_err());
}

#[test]
fn the_force_detached_control_writes_the_wire_object() {
    let control = ForceDetachedControl {
        reason: "workspace_gone".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&control).unwrap(),
        serde_json::json!({"event": "force_detached", "reason": "workspace_gone"})
    );
    assert_eq!(
        force_detach_notice(&serde_json::to_vec(&control).unwrap()),
        Some(ForceDetachNotice::WorkspaceGone)
    );
}

#[test]
fn a_force_detached_control_without_a_string_reason_is_final() {
    for payload in [
        r#"{"event":"force_detached"}"#,
        r#"{"event":"force_detached","reason":3}"#,
        r#"{"event":"force_detached","reason":"forced"}"#,
    ] {
        assert_eq!(
            force_detach_notice(payload.as_bytes()),
            Some(ForceDetachNotice::Final),
            "{payload}"
        );
    }
}
