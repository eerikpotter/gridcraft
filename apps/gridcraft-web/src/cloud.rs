//! Uses upstream's complete JSON workbook format, including formulas, styles and media.
use sovereign_cloud_ui::{DocumentState, Editor, Snapshot};

pub struct Adapter<'a>(pub &'a mut gridcraft_ui_egui::SheetApp);
impl Editor for Adapter<'_> {
    fn document(&self) -> Option<DocumentState> {
        self.0.session.active().map(|d| DocumentState {
            id: d.uid,
            revision: d.revision,
            name: d.display_title(),
            dirty: d.is_dirty() || self.0.editor.is_some() || self.0.text_box_editor.is_some(),
        })
    }
    fn capture(&self) -> Result<Snapshot, String> {
        if self.0.editor.is_some() || self.0.text_box_editor.is_some() {
            return Err("Finish or correct the cell being edited before saving to cloud".into());
        }
        let document = self.document().ok_or("No workbook is open")?;
        let wb = self.0.session.active().ok_or("No workbook is open")?.wb.clone();
        Ok(Snapshot { document, encode: Box::new(move || gridcraft_engine::io::save_bytes(&wb, "workbook.scjson").map_err(|e| e.to_string())) })
    }
    fn saved(&mut self, id: u64, revision: u64, _bytes: &[u8]) -> Result<Option<u64>, String> {
        if self.0.editor.is_none()
            && self.0.text_box_editor.is_none()
            && let Some(d) = self.0.session.active_mut()
            && d.uid == id
            && d.revision == revision
        {
            d.saved = d.wb.clone();
            return Ok(Some(revision));
        }
        Ok(None)
    }
    fn open(&mut self, name: &str, bytes: &[u8]) -> Result<DocumentState, String> {
        let (wb, _) = gridcraft_engine::io::open_bytes("workbook.scjson", bytes).map_err(|e| e.to_string())?;
        if !self.0.commit_edit(0, 0, false, false) {
            return Err("Finish or correct the cell being edited before opening another workbook".into());
        }
        self.0.commit_text_box_edit()?;
        self.0.session.add_document(gridcraft_engine::DocState::new(wb, None, name.into()));
        self.document().ok_or_else(|| "Could not open the workbook".into())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unfinished_cells_cannot_be_omitted_or_marked_saved() {
        let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        app.session.new_workbook();
        app.begin_edit(Some("=2+3".into()), false);
        assert!(Adapter(&mut app).document().unwrap().dirty);
        assert!(Adapter(&mut app).capture().is_err());
        assert!(app.commit_edit(0, 0, false, false));
        let snapshot = Adapter(&mut app).capture().unwrap();
        let current = snapshot.document;
        let bytes = (snapshot.encode)().unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("2+3"));
        app.begin_edit(Some("9".into()), false);
        assert_eq!(Adapter(&mut app).saved(current.id, current.revision, &bytes).unwrap(), None);
        assert!(Adapter(&mut app).document().unwrap().dirty);
    }
    #[test]
    fn cloud_snapshot_preserves_formulas_and_only_marks_its_revision_saved() {
        let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        app.session.new_workbook();
        app.session.run("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
        app.session.run("cell.set", json!({"cell": "B1", "input": "=A1*2"})).unwrap();
        let snapshot = Adapter(&mut app).capture().unwrap();
        let original = snapshot.document;
        app.session.run("cell.set", json!({"cell": "A1", "input": "99"})).unwrap();
        let bytes = (snapshot.encode)().unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("A1*2"));
        assert_eq!(Adapter(&mut app).saved(original.id, original.revision, &bytes).unwrap(), None);
        assert!(app.session.active().unwrap().is_dirty());
        let count = app.session.documents().len();
        assert!(Adapter(&mut app).open("bad.scjson", b"not json").is_err());
        assert_eq!(app.session.documents().len(), count);
        let opened = Adapter(&mut app).open("Workbook.scjson", &bytes).unwrap();
        assert_ne!(opened.id, original.id);
        let reopened = Adapter(&mut app).capture().unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::from_slice::<serde_json::Value>(&(reopened.encode)().unwrap()).unwrap()
        );
        assert!(app.session.documents().first().unwrap().is_dirty());
        app.session.run("cell.set", json!({"cell": "C1", "input": "8"})).unwrap();
        let current = Adapter(&mut app).document().unwrap();
        assert_eq!(Adapter(&mut app).saved(current.id, current.revision, &[]).unwrap(), Some(current.revision));
        assert!(!app.session.active().unwrap().is_dirty());
    }
}
