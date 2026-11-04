//! Turns a mod scan into reports: the terminal text `scan` and `report` print, and a
//! shareable, self-contained HTML page.

mod html;
mod text;

pub use html::page as html;
pub use text::{full as text_full, summary as text_summary};

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn fixture_scan() -> porter_model::ModScan {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../porter-java/tests/fixtures/neoforge-demo");
        porter_java::scan(&path).unwrap()
    }

    #[test]
    fn html_page_has_the_report() {
        let page = super::html(&fixture_scan());
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<title>Demo Machines 0.3.0 (demo) · horizon-porter report</title>"));
        assert!(page.contains("Needs these mods first: <code>flywheel</code>"));
        // The blocked @Redirect and the partial TAIL @Inject are listed with their notes.
        assert!(page.contains("@Redirect tick<wbr>Block<wbr>Entities at FIELD"));
        assert!(page.contains("horizon-mixins has no TAIL"));
        assert!(page.contains(">net.minecraft.world.level.block.Block</code>"));
        assert!(page.ends_with("</html>\n"));
        // Self-contained: no scripts and nothing loaded from elsewhere.
        assert!(!page.contains("<script"));
        assert!(!page.contains("<link"));
        assert!(!page.contains("src=\""));
    }

    #[test]
    fn escapes_text() {
        assert_eq!(
            super::html::esc(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }

    #[test]
    fn text_summary_still_works() {
        let text = super::text_summary(&fixture_scan());
        assert!(text.starts_with("Demo Machines 0.3.0 (demo) · NeoForge"));
        assert!(text.contains("Needs mods   flywheel"));
    }
}
