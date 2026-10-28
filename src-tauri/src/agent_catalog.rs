//! Curated CLI agents we can offer after a local detection pass.
//!
//! The island does not auto-add these: `detect_agent_candidates` only reports
//! the ones whose executable actually resolves on this machine, and the user
//! still has to press "添加". Entries without a log adapter are process-only;
//! they get no inherited resume/send commands.

pub(crate) struct CatalogAgent {
    pub name: &'static str,
    /// Executable name on PATH (or a well-known install name).
    pub program: &'static str,
    pub description: &'static str,
}

pub(crate) const CATALOG: &[CatalogAgent] = &[
    CatalogAgent {
        name: "Gemini CLI",
        program: "gemini",
        description: "Google Gemini CLI",
    },
    CatalogAgent {
        name: "Aider",
        program: "aider",
        description: "Aider pair programmer",
    },
    CatalogAgent {
        name: "Cursor Agent",
        program: "cursor-agent",
        description: "Cursor CLI agent",
    },
    CatalogAgent {
        name: "GitHub Copilot CLI",
        program: "copilot",
        description: "GitHub Copilot CLI",
    },
    CatalogAgent {
        name: "Qwen Code",
        program: "qwen",
        description: "Qwen Code CLI",
    },
    CatalogAgent {
        name: "Crush",
        program: "crush",
        description: "Charm Crush",
    },
    CatalogAgent {
        name: "Goose",
        program: "goose",
        description: "Block Goose",
    },
    CatalogAgent {
        name: "Amp",
        program: "amp",
        description: "Sourcegraph Amp",
    },
    CatalogAgent {
        name: "Droid",
        program: "droid",
        description: "Factory Droid",
    },
    CatalogAgent {
        name: "Kilo Code",
        program: "kilo",
        description: "Kilo Code CLI",
    },
    CatalogAgent {
        name: "Auggie",
        program: "auggie",
        description: "Augment Code CLI",
    },
    CatalogAgent {
        name: "Plandex",
        program: "plandex",
        description: "Plandex",
    },
    CatalogAgent {
        name: "OpenHands",
        program: "openhands",
        description: "OpenHands CLI",
    },
    CatalogAgent {
        name: "Cline",
        program: "cline",
        description: "Cline CLI",
    },
    CatalogAgent {
        name: "Warp Agent",
        program: "warp",
        description: "Warp Agent CLI",
    },
];
