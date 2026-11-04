//! Turns a mod scan into reports: the terminal text `scan` and `report` print, and a
//! shareable, self-contained HTML page.

mod html;
mod text;

pub use html::embedded as html_embedded;
pub use html::page as html;
pub use text::{full as text_full, summary as text_summary};

/// The mod's title (`Create 6.0.11 (create)`) and its facts (loaders, Minecraft
/// version, licence), for headers outside the report.
pub fn heading(scan: &porter_model::ModScan) -> (String, Vec<String>) {
    let f = text::facts(scan);
    let mut facts = Vec::new();
    facts.extend(f.loaders);
    facts.extend(f.minecraft.map(|m| format!("Minecraft {m}")));
    facts.extend(f.licence.map(|l| format!("Licence: {l}")));
    (f.title, facts)
}

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
        assert!(page.contains("<code title=\"net.minecraft.world.level.block.Block\">"));
        assert!(page.ends_with("</html>\n"));
        // Self-contained: no scripts and nothing loaded from elsewhere.
        assert!(!page.contains("<script"));
        assert!(!page.contains("<link"));
        assert!(!page.contains("src=\""));
    }

    #[test]
    fn embedded_page_leaves_the_header_to_the_app() {
        let scan = fixture_scan();
        let page = super::html_embedded(&scan);
        assert!(page.contains("<body class=\"embedded\">"));
        assert!(!page.contains("<header>"));
        assert!(!page.contains("<footer>"));
        // One slot per Mixin member, named on hover.
        let members: usize = scan.mixins.iter().map(|m| m.members.len()).sum();
        assert_eq!(page.matches("<i class=\"slot ").count(), members);
        assert!(page.contains("title=\"LevelMixin · @Redirect tickBlockEntities: blocked\""));
        let (title, facts) = super::heading(&scan);
        assert_eq!(title, "Demo Machines 0.3.0 (demo)");
        assert_eq!(facts, vec!["NeoForge", "Minecraft [26.3]", "Licence: MIT"]);
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
