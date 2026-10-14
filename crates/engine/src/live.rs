//! Per-document collaboration. Remote inputs are PDF data, never executable engine commands.
use crate::{DocId, Session};
use serde_json::Value;
use std::sync::Arc;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq)]
pub enum LiveChange {
    Op { cmd: String, params: Value, ctx: Value },
    Replace,
}

#[derive(Default)]
pub struct LiveState {
    pub document: Option<DocId>,
    on: bool,
    seen: u64,
}

impl Session {
    pub fn live_select(&mut self, id: DocId) {
        self.live.document = Some(id);
    }
    pub fn live_start(&mut self) -> Result<()> {
        let id = self.live.document.ok_or("Abra um PDF para colaborar.")?;
        let doc = self.get(id).ok_or("O PDF foi fechado.")?;
        if doc.read_only_reason.is_some() || doc.is_signed() {
            return Err("Este PDF é protegido ou assinado. Crie uma cópia editável para colaborar.".into());
        }
        self.live.seen = doc.generation;
        self.live.on = true;
        Ok(())
    }
    pub fn live_stop(&mut self) {
        self.live = LiveState::default();
    }
    pub fn live_active(&self) -> bool {
        self.live.on && self.live.document.is_some_and(|id| self.get(id).is_some())
    }
    pub fn live_busy(&self) -> bool {
        false
    }
    pub fn live_take(&mut self) -> Vec<LiveChange> {
        if self.live.on
            && let Some(doc) = self.live.document.and_then(|id| self.get(id))
            && doc.generation != self.live.seen
        {
            self.live.seen = doc.generation;
            return vec![LiveChange::Replace];
        }
        vec![]
    }
    pub fn live_snapshot(&self) -> Result<Vec<u8>> {
        let id = self.live.document.ok_or("Nenhum PDF compartilhado.")?;
        self.save_bytes(id).map(|b| (*b).clone()).map_err(|e| e.to_string())
    }
    pub fn live_digest(&self) -> Result<u64> {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for byte in self.live_snapshot()? {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(h)
    }
    pub fn live_load(&mut self, bytes: &[u8]) -> Result<()> {
        let id = self.live.document.ok_or("Nenhum PDF compartilhado.")?;
        // Remote documents must not run document-open JavaScript on this machine.
        let js_off = self.js_off;
        self.js_off = true;
        let result = self.reload(id, Arc::new(bytes.to_vec()));
        self.js_off = js_off;
        result.map_err(|e| e.to_string())?;
        let doc = self.docs.iter_mut().find(|d| d.id == id).ok_or("O PDF foi fechado.")?;
        doc.dirty = true;
        self.live.seen = doc.generation;
        Ok(())
    }
    pub fn live_apply(&mut self, _cmd: &str, _params: &Value, _ctx: &Value) -> Result<()> {
        Err("PDFs compartilhados aceitam somente alterações do documento.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(s: &mut Session, name: &str) -> DocId {
        let bytes = s.create_blank(300.0, 400.0, 1).unwrap();
        s.open(name, None, bytes, None).unwrap()
    }
    #[test]
    fn received_pdf_only_replaces_the_selected_document_and_stays_unsaved() {
        let mut s = Session::new();
        let private = document(&mut s, "Privado");
        let shared = document(&mut s, "Compartilhado");
        let before = s.save_bytes(private).unwrap();
        s.live_select(shared);
        s.live_start().unwrap();
        let incoming = s.create_blank(500.0, 600.0, 2).unwrap();
        s.live_load(&incoming).unwrap();
        assert_eq!(s.get(shared).unwrap().info.pages.len(), 2);
        assert!(s.get(shared).unwrap().dirty);
        assert_eq!(s.save_bytes(private).unwrap(), before);
        assert!(s.live_take().is_empty());
        assert!(s.live_apply("file.open", &Value::Null, &Value::Null).is_err());
    }
}
