//! Static prompts, skills, templates, and their cycle-specific composition.

use std::path::PathBuf;

use flaggo_analysis_domain::{AnalysisError, AnalysisProfile, AttemptContext};
use flaggo_analysis_workspace::WorkspaceFileSystem;

pub const ANALYSIS_SKILL_NAMES: &[&str] = &[
    "analysis-cycle-protocol",
    "understand-decision-contract",
    "qualitative-analysis",
    "author-executable",
];
pub const ANALYSIS_FINDINGS_PATH: &str = "/workspace/analysis/findings.md";
pub const SUPERSESSION_NOTE_PATH: &str = "/workspace/analysis/supersession.md";
pub const HANDOFF_CHECKPOINT_PATH: &str = "/workspace/handoffs/checkpoint.md";

pub const ANALYSIS_SYSTEM_PROMPT: &str = include_str!("../agent/prompts/system.md");
const ANALYSIS_TASK_PROMPT: &str = include_str!("../agent/prompts/task.md");
const ANALYSIS_FINDINGS_TEMPLATE: &str = include_str!("../agent/templates/findings.md");
const SUPERSESSION_NOTE_TEMPLATE: &str = include_str!("../agent/templates/supersession.md");
const HANDOFF_CHECKPOINT_TEMPLATE: &str = include_str!("../agent/templates/handoff.md");

#[must_use]
pub fn analysis_skill_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("agent")
        .join("skills")
}

#[must_use]
pub fn analysis_profile() -> AnalysisProfile {
    AnalysisProfile {
        skill_names: ANALYSIS_SKILL_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

#[must_use]
pub fn analysis_task_prompt(context: &AttemptContext) -> String {
    format!(
        "{ANALYSIS_TASK_PROMPT}\n\n\
         <bound-cycle>\n\
           <contract-name>{}</contract-name>\n\
           <contract-digest>{}</contract-digest>\n\
           <workspace-id>{}</workspace-id>\n\
           <cycle-id>{}</cycle-id>\n\
           <attempt-id>{}</attempt-id>\n\
         </bound-cycle>",
        xml_text(&context.contract.name),
        xml_text(&context.contract.digest),
        xml_text(&context.workspace_id),
        xml_text(&context.cycle_id),
        xml_text(&context.attempt_id)
    )
}

pub async fn prepare_agent_workspace(
    filesystem: &dyn WorkspaceFileSystem,
) -> Result<(), AnalysisError> {
    for (path, template) in [
        (ANALYSIS_FINDINGS_PATH, ANALYSIS_FINDINGS_TEMPLATE),
        (SUPERSESSION_NOTE_PATH, SUPERSESSION_NOTE_TEMPLATE),
        (HANDOFF_CHECKPOINT_PATH, HANDOFF_CHECKPOINT_TEMPLATE),
    ] {
        if !filesystem.exists(path).await? {
            filesystem.write_text(path, template).await?;
        }
    }
    Ok(())
}

fn xml_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use flaggo_analysis_domain::{AnalysisContractIdentity, AttemptContext};

    use super::{
        ANALYSIS_FINDINGS_TEMPLATE, ANALYSIS_SKILL_NAMES, ANALYSIS_SYSTEM_PROMPT,
        ANALYSIS_TASK_PROMPT, HANDOFF_CHECKPOINT_TEMPLATE, SUPERSESSION_NOTE_TEMPLATE,
        analysis_profile, analysis_skill_directory, analysis_task_prompt,
    };

    #[test]
    fn profile_binds_every_semantically_tagged_skill_by_name() {
        assert_eq!(
            analysis_profile().skill_names,
            ANALYSIS_SKILL_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );

        for (name, document, semantic_tags) in [
            (
                "analysis-cycle-protocol",
                include_str!("../agent/skills/analysis-cycle-protocol/SKILL.md"),
                &[
                    "cycle_context",
                    "cycle_status",
                    "evidence_snapshot",
                    "analysis_record",
                    "cycle_outcome",
                ][..],
            ),
            (
                "understand-decision-contract",
                include_str!("../agent/skills/understand-decision-contract/SKILL.md"),
                &["contract_structure", "learning_goal", "contract_executable"][..],
            ),
            (
                "qualitative-analysis",
                include_str!("../agent/skills/qualitative-analysis/SKILL.md"),
                &[
                    "analysis_question",
                    "evidence_quality",
                    "comparative_analysis",
                    "query_strategy",
                    "executable_decision",
                ][..],
            ),
            (
                "author-executable",
                include_str!("../agent/skills/author-executable/SKILL.md"),
                &[
                    "executable_constraints",
                    "expression_syntax",
                    "executable_rules",
                    "candidate_submission",
                ][..],
            ),
        ] {
            let discovery_metadata = format!("name: {name}");
            assert!(document.lines().any(|line| line == discovery_metadata));
            for tag in semantic_tags {
                assert_xml_block(document, tag);
            }
        }
    }

    #[test]
    fn assets_combine_markdown_headings_with_semantic_xml_blocks() {
        for tag in [
            "closed_loop_system",
            "analysis_mission",
            "decision_contract_requirements",
        ] {
            assert_xml_block(ANALYSIS_SYSTEM_PROMPT, tag);
        }
        assert_xml_block(ANALYSIS_TASK_PROMPT, "analysis_assignment");

        for (template, tags) in [
            (
                ANALYSIS_FINDINGS_TEMPLATE,
                &[
                    "questions_and_queries",
                    "observed_findings",
                    "interpretation_and_uncertainty",
                    "objective_and_guardrails",
                    "executable_conclusion",
                ][..],
            ),
            (
                SUPERSESSION_NOTE_TEMPLATE,
                &["reusable_work", "remaining_uncertainty"][..],
            ),
            (
                HANDOFF_CHECKPOINT_TEMPLATE,
                &["completed_work", "next_step"][..],
            ),
        ] {
            assert!(template.contains("# "));
            for tag in tags {
                assert_xml_block(template, tag);
            }
        }
    }

    #[test]
    fn task_prompt_xml_binds_escaped_identity_without_repeating_the_workflow() {
        let prompt = analysis_task_prompt(&AttemptContext {
            workspace_id: "workspace<&".to_owned(),
            cycle_id: "cycle".to_owned(),
            attempt_id: "attempt".to_owned(),
            contract: AnalysisContractIdentity {
                name: "checkout.delay".to_owned(),
                digest: "sha256:\"digest\"".to_owned(),
            },
        });

        assert!(prompt.contains("<contract-name>checkout.delay</contract-name>"));
        assert!(prompt.contains("<contract-digest>sha256:&quot;digest&quot;</contract-digest>"));
        assert!(prompt.contains("<workspace-id>workspace&lt;&amp;</workspace-id>"));
        assert!(prompt.contains("Load `analysis-cycle-protocol`"));
        assert_xml_block(&prompt, "analysis_assignment");
        assert_xml_block(&prompt, "bound-cycle");
        assert!(!prompt.contains("commit_cutoff"));
        assert!(!prompt.contains("propose_executable"));
    }

    #[test]
    fn skill_directory_is_owned_by_the_analysis_agent_module() {
        let skill_directory = analysis_skill_directory();

        assert!(skill_directory.ends_with("agent/skills"));
        assert!(skill_directory.is_dir());
    }

    #[cfg(feature = "copilot")]
    #[test]
    fn system_defines_role_while_protocol_owns_operations() {
        use crate::copilot::tools::REGISTERED_ANALYSIS_TOOL_NAMES;

        const PROTOCOL: &str = include_str!("../agent/skills/analysis-cycle-protocol/SKILL.md");

        assert!(ANALYSIS_SYSTEM_PROMPT.contains("data scientist"));
        assert!(ANALYSIS_SYSTEM_PROMPT.contains("closed-loop decision system"));
        assert!(ANALYSIS_SYSTEM_PROMPT.contains("Achieve its objective"));
        assert!(ANALYSIS_SYSTEM_PROMPT.contains("satisfy every guardrail"));
        for implementation_detail in [
            "workspace",
            "database",
            "shell",
            "network",
            "host filesystem",
        ] {
            assert!(
                !ANALYSIS_SYSTEM_PROMPT.contains(implementation_detail),
                "system prompt must not describe the {implementation_detail} boundary"
            );
        }
        for tool_name in REGISTERED_ANALYSIS_TOOL_NAMES {
            assert!(
                !ANALYSIS_SYSTEM_PROMPT.contains(tool_name),
                "system prompt must not encode the {tool_name} workflow"
            );
            assert!(
                PROTOCOL.contains(tool_name),
                "protocol skill must describe {tool_name}"
            );
        }
    }

    fn assert_xml_block(document: &str, tag: &str) {
        assert!(document.contains(&format!("<{tag}>")));
        assert!(document.contains(&format!("</{tag}>")));
    }
}
