//! Android adapter contracts with owned adb fixtures and pure hierarchy checks.
use super::*;
use super::{
    adb::{quote, raw_png, secure_focus},
    input::pixels,
};
use serde_json::json;
use std::time::Duration;
#[test]
fn raw_captures_become_pngs_and_unknown_layouts_are_left_to_the_device() {
    let raw = |header: &[u32], pixels: &[u8]| {
        let mut bytes: Vec<u8> = header.iter().flat_map(|word| word.to_le_bytes()).collect();
        bytes.extend_from_slice(pixels);
        bytes
    };
    let pixels: Vec<u8> = (0..2 * 3).flat_map(|n| [n * 10, 1, 2, 3]).collect();
    // Newer releases add a color space word; older ones end after the format.
    for header in [&[2, 3, 1, 0][..], &[2, 3, 1]] {
        let png = raw_png(&raw(header, &pixels)).unwrap();
        assert_eq!(crate::ui::png_dimensions(&png).unwrap(), (2, 3));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(1, 0).0, [10, 1, 2, 3]);
    }
    // RGBX pixels carry no alpha, so they come out opaque.
    let opaque = image::load_from_memory(&raw_png(&raw(&[2, 3, 2], &pixels)).unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(opaque.get_pixel(0, 0).0[3], 255);
    let png = raw_png(&raw(&[2, 3, 1], &pixels)).unwrap();
    for unknown in [
        png.clone(),                    // already a PNG
        raw(&[2, 3, 4], &pixels),       // another pixel format
        raw(&[2, 3, 1], &pixels[..20]), // truncated pixels
        raw(&[0, 3, 1], &[]),           // no width
        Vec::new(),
    ] {
        assert!(raw_png(&unknown).is_none());
    }
}
#[test]
fn package_read_keeps_home_screen_names_and_refuses_bad_frames() {
    let frame = br#"framework chatter
{"version":1,"ok":true,"data":{"apps":[{"appId":"com.example.zeta","name":"Zeta","version":"2.0","build":"20","launchable":false},{"appId":"com.example.acme.game","name":"Acme Quest","version":"1.2.3","build":"7","launchable":true}]}}
"#;
    let inventory = super::apps::named_inventory(frame).unwrap();
    assert_eq!(inventory["apps"][0]["appId"], "com.example.acme.game");
    assert_eq!(inventory["apps"][0]["name"], "Acme Quest");
    assert_eq!(inventory["apps"][0]["version"], "1.2.3");
    assert!(inventory.get("unavailableFields").is_none());
    for bad in [
        &br#"{"version":1,"ok":false,"error":"java.lang.SecurityException"}"#[..],
        br#"{"version":1,"ok":true,"data":{"apps":[]}}"#,
        br#"{"version":1,"ok":true,"data":{"apps":[{"appId":"bad id","name":"x","launchable":true}]}}"#,
        br#"{"version":1,"ok":true,"data":{"apps":[{"appId":"com.example.acme","launchable":true}]}}"#,
        b"not json",
    ] {
        assert!(super::apps::named_inventory(bad).is_err());
    }
}
#[test]
fn only_a_focused_secure_window_explains_a_blocked_capture() {
    // Reconstructed from Android 12's WindowState.dump / LayoutParams.toString;
    // this is a format fixture, not a captured device dump.
    let dump = include_str!("../../tests/fixtures/android12-window-symbolic.txt");
    assert_eq!(secure_focus(dump).as_deref(), Some("com.example.acme"));
    let inline = dump.replace("\n      fl=", " fl=");
    assert_eq!(secure_focus(&inline).as_deref(), Some("com.example.acme"));
    let numeric = dump.replace(
        "KEEP_SCREEN_ON SECURE LAYOUT_IN_SCREEN HARDWARE_ACCELERATED",
        "#2000",
    );
    assert_eq!(secure_focus(&numeric).as_deref(), Some("com.example.acme"));
    // A secure window that is not focused does not explain the capture.
    let other_focus = dump.replace(
        "mCurrentFocus=Window{abc002 u0 com.example.acme/com.example.acme.Main}",
        "mCurrentFocus=Window{abc001 u0 StatusBar}",
    );
    assert_eq!(secure_focus(&other_focus), None);
    assert_eq!(secure_focus(&dump.replace(" SECURE", "")), None);
    assert_eq!(secure_focus("no window records"), None);
}
#[cfg(unix)]
#[tokio::test]
async fn gestures_follow_portrait_and_landscape_capture_dimensions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let adb = directory.path().join("adb");
    std::fs::write(
        &adb,
        "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\ncase \"$3\" in\nexec-out) cat screen.png ;;\nshell) case \"$4\" in\n\"'dumpsys' 'input'\") cat viewport.txt 2>/dev/null ;;\n*) printf '%s\\n' \"$4\" >> input.log ;;\nesac ;;\n*) exit 1 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let adapter = Android { adb };
    // Portrait reads the input service's viewport; landscape has none, so the
    // size falls back to the capture.
    for (width, height, viewport) in [(108, 240, true), (240, 108, false)] {
        image::RgbImage::new(width, height)
            .save(directory.path().join("screen.png"))
            .unwrap();
        let viewport_file = directory.path().join("viewport.txt");
        if viewport {
            std::fs::write(
                &viewport_file,
                format!("  Viewport INTERNAL: displayId=0, orientation=0, logicalFrame=[0, 0, {width}, {height}]\n"),
            )
            .unwrap();
        } else {
            let _ = std::fs::remove_file(&viewport_file);
        }
        assert_eq!(
            adapter.dimensions("owned-fixture").await.unwrap(),
            (width, height)
        );
        adapter
            .tap("owned-fixture", [0.75, 0.5], None, Duration::from_secs(2))
            .await
            .unwrap();
        adapter
            .swipe(
                "owned-fixture",
                [[0.0, 0.0], [1.0, 1.0]],
                100,
                None,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("input.log")).unwrap(),
        "'input' 'tap' '80' '120'\n'input' 'swipe' '0' '0' '107' '239' '100'\n'input' 'tap' '179' '54'\n'input' 'swipe' '0' '0' '239' '107' '100'\n"
    );
    std::fs::remove_file(directory.path().join("input.log")).unwrap();
    let stale = crate::ui::Display {
        display_id: "main".into(),
        geometry_version: "108x240".into(),
    };
    assert_eq!(
        adapter
            .tap(
                "owned-fixture",
                [0.5, 0.5],
                Some(&stale),
                Duration::from_secs(2)
            )
            .await
            .unwrap_err()
            .code,
        "STALE_GEOMETRY"
    );
    assert_eq!(
        adapter
            .swipe(
                "owned-fixture",
                [[0.0, 0.0], [1.0, 1.0]],
                100,
                Some(&stale),
                Duration::from_secs(2)
            )
            .await
            .unwrap_err()
            .code,
        "STALE_GEOMETRY"
    );
    assert!(!directory.path().join("input.log").exists());
}
#[test]
fn xml_queries_preserve_structure_and_omit_secure_values() {
    let xml=br#"UI dump: <hierarchy><node text="Acme &amp; example" resource-id="qa.title" class="android.widget.TextView" bounds="[0,0][100,50]"><node text="fixture-secret-example" password="true" resource-id="qa.password" class="android.widget.EditText" bounds="[0,50][100,100]"></node></node></hierarchy>"#;
    let result = hierarchy(xml).unwrap();
    assert_eq!(result["nodeCount"], 2);
    assert_eq!(result["root"]["children"][0]["label"], "Acme & example");
    assert!(
        result["root"]["children"][0]["children"][0]
            .get("value")
            .is_none()
    );
    assert!(!result.to_string().contains("fixture-secret-example"));
    assert!(hierarchy(b"<hierarchy><node bounds=\"bad\"/></hierarchy>").is_err());
    let offscreen =
        hierarchy(b"<hierarchy><node bounds=\"[0,2368][1080,2337]\"/></hierarchy>").unwrap();
    assert_eq!(
        offscreen["root"]["children"][0]["frame"]["available"],
        false
    );
    assert_eq!(quote("a'b;$()"), "'a'\\''b;$()'");
    assert!(pixels([f64::NAN, 0.0], 100, 100).is_err());
}
#[test]
fn widget_roles_share_the_common_vocabulary_and_keep_native_classes() {
    let xml = br#"<hierarchy><node class="android.widget.ScrollView" scrollable="true" bounds="[0,0][100,200]"><node class="android.widget.EditText" focused="true" text="Acme" bounds="[0,0][100,20]"/><node class="android.widget.CheckBox" checkable="true" checked="false" bounds="[0,20][100,40]"/><node class="android.widget.SeekBar" bounds="[0,40][100,60]"/><node class="android.widget.ImageView" bounds="[0,60][100,80]"/><node class="android.widget.Switch" bounds="[0,80][100,100]"/><node class="android.widget.ImageButton" bounds="[0,100][100,120]"/><node class="android.widget.LinearLayout" bounds="[0,120][100,140]"/></node></hierarchy>"#;
    let mut android = hierarchy(xml).unwrap()["root"].clone();
    let scroll = &android["children"][0];
    assert_eq!(scroll["role"], "scroll_view");
    assert_eq!(scroll["states"]["scrollable"], true);
    let roles: Vec<_> = scroll["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["role"].clone(), node["nativeRole"].clone()))
        .collect();
    assert_eq!(
        roles,
        [
            ("text_field", "EditText"),
            ("checkbox", "CheckBox"),
            ("slider", "SeekBar"),
            ("image", "ImageView"),
            ("switch", "Switch"),
            ("button", "ImageButton"),
            ("element", "LinearLayout"),
        ]
        .map(|(role, native)| (json!(role), json!(native)))
    );
    assert_eq!(scroll["children"][0]["value"], "Acme");
    assert!(crate::ui::focused_editable(&android));
    crate::ui::normalize_roles(&mut android);
    assert_eq!(android["children"][0]["children"][0]["role"], "text_field");
    let before = android.clone();
    crate::ui::normalize_roles(&mut android);
    assert_eq!(android, before);
    let mut ios = json!({"role":"AXApplication","children":[{"role":"AXScrollArea","children":[{"role":"AXTextField"},{"role":"AXSlider"},{"role":"AXImage"},{"role":"AXSwitch"},{"role":"AXButton"}]}]});
    crate::ui::normalize_roles(&mut ios);
    for role in [
        "scroll_view",
        "text_field",
        "slider",
        "image",
        "switch",
        "button",
    ] {
        let query: crate::ui::Query = serde_json::from_value(json!({"role":role})).unwrap();
        for root in [&android, &ios] {
            assert_eq!(
                query.find(root, 10).unwrap()["matches"]
                    .as_array()
                    .unwrap()
                    .len(),
                1,
                "{role} in {root}"
            );
        }
    }
    for native in ["SeekBar", "LinearLayout", "checkbox"] {
        let query: crate::ui::Query = serde_json::from_value(json!({"role":native})).unwrap();
        assert_eq!(
            query.find(&android, 10).unwrap()["matches"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{native}"
        );
    }
}
#[test]
fn focused_package_requires_an_app_activity_window() {
    use super::apps::focused_package;
    assert_eq!(
        focused_package(
            "  mCurrentFocus=Window{1a2b3c u0 com.example.acme/com.example.acme.MainActivity}\n"
        )
        .as_deref(),
        Some("com.example.acme")
    );
    for text in [
        "  mCurrentFocus=null",
        "  mCurrentFocus=Window{1a2b3c u0 Splash Screen com.example.acme}",
        "  mCurrentFocus=Window{1a2b3c u0 NotificationShade}",
        "",
    ] {
        assert_eq!(focused_package(text), None, "{text}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn long_text_is_typed_in_ordered_chunks_each_with_its_own_event_time() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let adb = directory.path().join("adb");
    std::fs::write(
        &adb,
        "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n[ \"$3\" = shell ] || exit 1\nprintf '%s\\n' \"$4\" >> input.log\n",
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let adapter = Android { adb };
    let text = format!("{} {}", "a".repeat(40), "\"Acme\"!".repeat(5));
    let reply = adapter
        .type_text("owned-fixture", &text, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(reply["chunks"], 3);
    let log = std::fs::read_to_string(directory.path().join("input.log")).unwrap();
    let typed: String = log
        .lines()
        .map(|line| {
            let words: Vec<_> = line.split("' '").collect();
            assert_eq!(&words[..2], ["'input", "text"]);
            words[2].trim_end_matches('\'').replace("%s", " ")
        })
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(log.lines().count(), 3);
    assert_eq!(typed, text);
}

#[test]
fn typing_limit_and_default_deadline_follow_text_length() {
    assert!(super::text_argument(&"a".repeat(2000)).is_ok());
    assert_eq!(
        super::text_argument(&"a".repeat(2001)).unwrap_err().code,
        "INVALID_ARGUMENT"
    );
    assert_eq!(super::text_timeout(""), Duration::from_secs(15));
    // 1,320 characters timed out at 30 s on a phone typing ~24 per second (~55 s).
    assert!(super::text_timeout(&"a".repeat(1320)) >= Duration::from_secs(80));
    assert!(super::text_timeout(&"a".repeat(2000)) < Duration::from_secs(120));
}

#[test]
fn a_cut_field_value_is_marked_on_its_element_and_keeps_the_tree_complete() {
    let start = format!("{} Acme", "a".repeat(1019));
    let xml = format!(
        r#"<hierarchy scope="active_window" window-id="1" truncated="false"><node class="android.widget.EditText" resource-id="com.example.acme:id/note" text="{start}" text-truncated="true" bounds="[0,0][100,40]"/><node class="android.widget.Button" text="Save" bounds="[0,40][100,80]"/></hierarchy>"#
    );
    let tree = super::hierarchy(xml.as_bytes()).unwrap();
    assert_eq!(tree["quality"], "complete");
    let note = &tree["root"]["children"][0];
    assert_eq!(note["valueTruncated"], true);
    assert_eq!(note["labelTruncated"], true);
    let query =
        |value: serde_json::Value| -> crate::ui::Query { serde_json::from_value(value).unwrap() };
    // A substring in the kept start proves the match.
    let found = crate::ui::semantic::unique(
        &query(json!({"value":{"text":"Acme","mode":"substring"}})),
        &tree,
    )
    .unwrap();
    assert_eq!(found["identifier"], "com.example.acme:id/note");
    // An exact label that fits in the kept start cannot be the longer value.
    assert_eq!(
        crate::ui::semantic::unique(
            &query(json!({"label":{"text":"Save","mode":"exact"}})),
            &tree
        )
        .unwrap()["label"],
        "Save"
    );
    // Text that may follow the cut leaves absence and uniqueness unproven.
    assert_eq!(
        crate::ui::semantic::unique(
            &query(json!({"value":{"text":"Acme end","mode":"substring"}})),
            &tree
        )
        .unwrap_err()
        .code,
        "INCONCLUSIVE"
    );
    let ends: crate::ui::semantic::Condition = serde_json::from_value(
        json!({"kind":"value","query":{"identifier":"com.example.acme:id/note"},"value":{"text":"Acme end","mode":"substring"}}),
    )
    .unwrap();
    assert!(!ends.evaluate(&tree).unwrap().0);
}

#[test]
fn package_inventory_reports_version_codes_and_unavailable_metadata() {
    let inventory = super::apps::inventory(
        b"package:com.example.zeta versionCode:42\npackage:com.example.acme versionCode:7\npackage:com.example.legacy\n",
    )
    .unwrap();
    assert_eq!(
        inventory["apps"],
        json!([
            {"appId":"com.example.acme","name":"","version":"","build":"7"},
            {"appId":"com.example.legacy","name":"","version":"","build":""},
            {"appId":"com.example.zeta","name":"","version":"","build":"42"},
        ])
    );
    assert_eq!(inventory["unavailableFields"], json!(["name", "version"]));
    assert!(super::apps::inventory(b"package:com.example.acme;rm\n").is_err());
}
#[test]
fn native_tree_limits_prevent_negative_and_unique_proofs() {
    let tree = super::hierarchy(br#"<hierarchy scope="active_window" truncated="true"><node class="android.widget.Button" text="Home" bounds="[0,0][100,40]"/></hierarchy>"#).unwrap();
    assert_eq!(tree["quality"], "partial");
    assert_eq!(tree["coverage"], "active_window_exposed_tree");
    let query = serde_json::from_value(serde_json::json!({"label":{"text":"Home","mode":"exact"}}))
        .unwrap();
    assert_eq!(
        crate::ui::semantic::unique(&query, &tree).unwrap_err().code,
        "UNIQUENESS_UNPROVEN"
    );
    let gone: crate::ui::semantic::Condition = serde_json::from_value(
        serde_json::json!({"kind":"gone","query":{"identifier":"acme.missing"}}),
    )
    .unwrap();
    assert!(!gone.evaluate(&tree).unwrap().0);
}

#[test]
fn renumbered_native_window_keeps_a_reference_but_changed_nodes_refuse_it() {
    let xml = r#"<hierarchy scope="active_window" window-id="1"><node class="android.widget.Button" text="Home" bounds="[0,0][100,40]"/></hierarchy>"#;
    let old = super::hierarchy(xml.as_bytes()).unwrap();
    let renumbered =
        super::hierarchy(xml.replace("window-id=\"1\"", "window-id=\"2\"").as_bytes()).unwrap();
    assert_eq!(
        crate::ui::semantic::revalidate(&old, &renumbered, "node-1").unwrap()["label"],
        "Home"
    );
    let changed = super::hierarchy(xml.replace("Home", "Other").as_bytes()).unwrap();
    assert_eq!(
        crate::ui::semantic::revalidate(&old, &changed, "node-1")
            .unwrap_err()
            .code,
        "STALE_ELEMENT"
    );
    let missing = super::hierarchy(
        xml.replace("window-id=\"1\"", "window-id=\"-1\"")
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(missing["quality"], "partial");
}
