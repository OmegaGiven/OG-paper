//! Text boxes (Text › Text box): the words wrap inside a box and get
//! smaller to fit it. The object is `ObjData::TextBox`; its layout is
//! `objects::textbox_fit`. These are its tests.

#[cfg(test)]
mod tests {
    use crate::objects::{textbox_fit, ObjData, Op, TextStyle};
    use crate::App;

    #[test]
    fn words_wrap_to_the_width_and_shrink_to_fit_the_height() {
        let st = TextStyle::default();
        // Wide and tall enough: one line, at the size asked for.
        let (lines, size) = textbox_fit("Hello there", &st, 1000.0, 200.0, 20.0);
        assert_eq!((lines.as_str(), size), ("Hello there", 20.0));
        // Narrow: wrapped, still at full size.
        let (lines, size) = textbox_fit("one two three four", &st, 90.0, 400.0, 20.0);
        assert!(lines.lines().count() >= 2, "{lines:?}");
        assert_eq!(size, 20.0);
        // Too little room: smaller, and it fits.
        let text = "a long run of words that cannot fit at this size in so small a box";
        let (lines, size) = textbox_fit(text, &st, 120.0, 40.0, 20.0);
        assert!(size < 20.0 && size > 0.5, "{size}");
        let b = crate::objects::text_box(&lines, &st, size);
        assert!(b[0] <= 120.0 * 1.01 && b[1] <= 40.0 * 1.01, "{b:?} {lines:?}");
        // The words are all there, in order.
        assert_eq!(lines.split_whitespace().collect::<Vec<_>>().join(" "), text);
    }

    #[test]
    fn a_dragged_out_box_types_wraps_resizes_and_saves() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.size_override = Some([800.0, 600.0]);
        app.ui.tool = crate::ui::Tool::Text;
        app.ui.text_box = true;
        app.ui.text_box_border = Some(crate::shapes::ShapeStyle::default());
        app.textbox_begin([100.0, 100.0], [300.0, 200.0]);
        let t = app.edit.text.as_mut().expect("typing");
        t.text = "Words that wrap inside the box you drew".into();
        app.ui.text_edit = Some((egui::pos2(0.0, 0.0), t.text.clone()));
        app.text_commit();
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert_eq!(list[0]["kind"], "textbox", "{list}");
        assert!(list[0]["border"].is_string(), "{list}");
        let g = app.objs.groups.len() as u32 - 1;
        let ObjData::TextBox { geom, size, .. } = app.objs.groups[g as usize].data.clone() else {
            panic!("a text box");
        };
        // Resized to half its width: the box changes, the size it starts
        // from stays (the words refit).
        let d = app.objs.groups[g as usize].data.edited(&Op::Scale {
            pivot: geom.center,
            sx: 0.5,
            sy: 1.0,
            rot: 0.0,
        });
        let ObjData::TextBox { geom: g2, size: s2, .. } = &d else {
            panic!("a text box");
        };
        assert!((g2.half[0] - geom.half[0] * 0.5).abs() < 1e-9 && *s2 == size);
        // Saved and read back: still a text box, words and border kept.
        let bytes = crate::snapshot::encode(
            &app.scene,
            &app.cam,
            &app.timeline,
            &[],
            &app.objs,
            &app.share.copy_log(),
        );
        let snap = crate::snapshot::decode(&bytes, 800.0).unwrap();
        assert!(snap.objs.groups.iter().any(|g| matches!(&g.data,
            ObjData::TextBox { text, border: Some(_), .. } if text.starts_with("Words that wrap"))));
        // As a lone object (files, the library, toolbars) too.
        let lone = crate::snapshot::data_bytes(&app.objs.groups[g as usize].data);
        assert!(matches!(
            crate::snapshot::get_data(&lone).unwrap(),
            ObjData::TextBox { border: Some(_), .. }
        ));
        // An older app reads the words as wrapped, as a text.
        let old = app.objs.groups[g as usize].data.textbox_as_text().unwrap();
        assert!(matches!(old, ObjData::Text { text, .. } if text.contains('\n')));
    }

    #[test]
    fn scripts_add_text_boxes() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.run_commands(&serde_json::json!([
            {"add": "textbox", "text": "Hi there", "x": -100, "y": -50, "w": 200, "h": 100, "border": "#336699", "fill": "#eeeeee"}
        ]));
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert_eq!(list[0]["kind"], "textbox");
        assert_eq!(list[0]["border"], "#336699");
        assert_eq!(list[0]["text"], "Hi there");
    }
}
