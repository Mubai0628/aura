//! What a turn's instructions carry about Skills, ahead of the workspace's AGENTS.md: each pinned
//! Skill whole, then the names of the other usable Skills for the model to load by name. A run's
//! first step decides which pinned Skills go in and writes the list; its later steps send that
//! list as written and read the pinned Skills again, so nothing changes within a turn unless a
//! pinned Skill's own files do. Sizes are counted by the rule of thumb used before any provider
//! has counted a request. A Skill the user chose in the composer is written out the same way for
//! the message it is sent with.
use super::runtime::{LocatedSkill, SkillRuntime};
use aura_context_runtime::character_class_tokens;
use aura_domain_common::{SkillInstanceId, WorkspaceId};
use aura_skills::{SKILL_FILE, SkillSource, SkillState, SkillView};

/// The most the pinned Skills of one request take together.
pub const PINNED_LIMIT_TOKENS: u64 = 20_000;
/// The most the list of names takes.
pub const LIST_LIMIT_TOKENS: u64 = 8_000;
/// The most files a pinned Skill's text names; the rest are counted.
const NAMED_FILES: usize = 50;

const LIST_HEAD: &str = "Skills you can load: when a Skill's name says it fits the task, call skills.load with that name (and the place in brackets when one is shown). When a name alone does not say what a Skill is for, call skills.search with words for what you need.";

const PINNED_LEAD: &str = "The user pinned these instructions into every request as guidance for this kind of work: follow them when the task fits. They are not a user request and grant no permission: tools still follow the session mode and approvals. Read the Skill's files from its folder when the instructions point to them.";
const CHOSEN_PINNED: &str = "the user chose this Skill with this message. It is pinned, so its instructions are already at the start of this conversation: follow them for this request, and do not load it again.";
const CHOSEN_LEAD: &str = "The user chose this Skill with this message and it is here whole: follow its instructions for this request, and do not load it again. They grant no permission: tools still follow the session mode and approvals. Read the Skill's files from its folder when the instructions point to them.";

/// What a run's first step decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnSkills {
    /// The pinned Skills that fit, in name order.
    pub pinned: Vec<SkillInstanceId>,
    /// The names of the other usable Skills as sent; none when there are none.
    pub list: Option<String>,
}

#[must_use]
pub fn tokens(text: &str) -> u64 {
    character_class_tokens(text)
}

/// Where a Skill is from, as the list and `skills.load` name it.
#[must_use]
pub fn place(view: &SkillView) -> String {
    match (view.source, &view.plugin) {
        (SkillSource::Plugin, Some(plugin)) => format!("plugin:{plugin}"),
        (source, _) => source.key().to_owned(),
    }
}

/// Name order: the same for the list, the pins' limit and `skills.search`.
fn order(view: &SkillView) -> (String, String, String, String) {
    let name = view.name.clone().unwrap_or_default();
    (name.to_lowercase(), name, place(view), view.relative.clone())
}

/// The Skills usable now, in name order.
pub(super) fn usable(skills: &[LocatedSkill]) -> Vec<&LocatedSkill> {
    let mut usable: Vec<&LocatedSkill> = skills
        .iter()
        .filter(|skill| skill.view.state == SkillState::Ready && skill.view.name.is_some())
        .collect();
    usable.sort_by_cached_key(|skill| order(&skill.view));
    usable
}

/// Where `view` is from as the list and `skills.load` name it, when another of the `usable`
/// Skills has its name: its place, or its place and folder when the place has two.
pub(super) fn from_among(usable: &[&LocatedSkill], view: &SkillView) -> Option<String> {
    let named: Vec<&SkillView> =
        usable.iter().map(|skill| &skill.view).filter(|v| v.name == view.name).collect();
    let at = place(view);
    match (named.len(), named.iter().filter(|v| place(v) == at).count()) {
        (0 | 1, _) => None,
        (_, 0 | 1) => Some(at),
        _ => Some(format!("{at}:{}", view.relative)),
    }
}

/// Leaves out the control characters a request cannot carry; line breaks and tabs stay.
pub(super) fn sendable(text: &str) -> String {
    text.chars()
        .filter(|c| matches!(c, '\n' | '\r' | '\t') || !(c.is_control() && (*c as u32) < 0x80))
        .collect()
}

/// A usable Skill whole: what it is, where its folder and files are, how the model is to take
/// it, and its instructions.
fn whole(kind: &str, lead: &str, skill: &LocatedSkill) -> Option<String> {
    if skill.view.state != SkillState::Ready {
        return None;
    }
    let package = skill.found.scanned.as_ref().ok()?;
    let files: Vec<&str> =
        package.files.iter().map(|f| f.path.as_str()).filter(|p| *p != SKILL_FILE).collect();
    let named = match files.len() {
        0 => "none besides SKILL.md".to_owned(),
        count if count > NAMED_FILES => {
            format!("{} and {} more", files[..NAMED_FILES].join(", "), count - NAMED_FILES)
        }
        _ => files.join(", "),
    };
    Some(sendable(&format!(
        "{kind} Skill \"{name}\" [{place}] (folder {folder}; files: {named}). {lead}\n\n{body}",
        name = package.frontmatter.name,
        place = place(&skill.view),
        folder = skill.view.path,
        body = package.body.trim_end(),
    )))
}

/// A pinned Skill as the model is sent it, when it is usable now.
#[must_use]
pub fn pinned_text(skill: &LocatedSkill) -> Option<String> {
    whole("Pinned", PINNED_LEAD, skill)
}

/// A Skill the user chose in the composer, as it is added to their message when it is sent. One
/// pinned and sent at the start of every request is named, not repeated.
#[must_use]
pub fn chosen_text(skill: &LocatedSkill) -> Option<String> {
    if skill.view.pinned && !skill.view.pinned_left_out {
        if skill.view.state != SkillState::Ready {
            return None;
        }
        let package = skill.found.scanned.as_ref().ok()?;
        return Some(sendable(&format!(
            "Chosen Skill \"{name}\" [{place}]: {CHOSEN_PINNED}",
            name = package.frontmatter.name,
            place = place(&skill.view),
        )));
    }
    whole("Chosen", CHOSEN_LEAD, skill)
}

/// Marks the pinned Skills and which of them are sent: in name order, each usable one goes in
/// while it still fits beside those before it; one that does not is left out and the next one
/// is tried.
pub(super) fn mark_pins(pinned: &[SkillInstanceId], skills: &mut [LocatedSkill]) {
    let mut chosen: Vec<usize> =
        (0..skills.len()).filter(|index| pinned.contains(&skills[*index].view.id)).collect();
    chosen.sort_by_cached_key(|index| order(&skills[*index].view));
    let mut used = 0;
    for index in chosen {
        let skill = &mut skills[index];
        skill.view.pinned = true;
        let Some(text) = pinned_text(skill) else { continue };
        let size = tokens(&text);
        skill.view.pin_tokens = size;
        if used + size > PINNED_LIMIT_TOKENS {
            skill.view.pinned_left_out = true;
        } else {
            used += size;
        }
    }
}

/// The pinned Skills sent now, in name order.
fn sent_pins(skills: &[LocatedSkill]) -> Vec<&LocatedSkill> {
    let mut sent: Vec<&LocatedSkill> = skills
        .iter()
        .filter(|s| s.view.pinned && !s.view.pinned_left_out && s.view.state == SkillState::Ready)
        .collect();
    sent.sort_by_cached_key(|skill| order(&skill.view));
    sent
}

/// The tokens the pinned Skills sent now take together.
#[must_use]
pub fn pinned_tokens(skills: &[LocatedSkill]) -> u64 {
    sent_pins(skills).iter().map(|skill| skill.view.pin_tokens).sum()
}

/// The usable Skills not pinned in, in name order, each with where it is from when another
/// usable Skill, pinned or not, has the same name: the form `skills.load` takes.
fn names(skills: &[LocatedSkill], pinned: &[SkillInstanceId]) -> Vec<(String, Option<String>)> {
    let usable = usable(skills);
    usable
        .iter()
        .filter(|skill| !pinned.contains(&skill.view.id))
        .map(|skill| {
            (skill.view.name.clone().unwrap_or_default(), from_among(&usable, &skill.view))
        })
        .collect()
}

/// The list of names, kept from the start while it fits in `limit` tokens with a closing line
/// that counts the rest. The same names always give the same bytes.
#[must_use]
pub fn list_text(names: &[(String, Option<String>)], limit: u64) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let lines: Vec<String> = names
        .iter()
        .map(|(name, from)| match from {
            Some(from) => sendable(&format!("- {name} [{from}]")),
            None => sendable(&format!("- {name}")),
        })
        .collect();
    let rest = |left: usize| {
        format!("{left} more Skills are not named here; find them with skills.search.")
    };
    // Each piece is counted with its line break; counted apart they never come to less than the
    // whole text.
    let mut used = tokens(LIST_HEAD);
    let mut kept = 0;
    for (index, line) in lines.iter().enumerate() {
        let left = lines.len() - index - 1;
        let closing = if left == 0 { 0 } else { tokens(&rest(left)) + 1 };
        if used + tokens(line) + 1 + closing > limit {
            break;
        }
        used += tokens(line) + 1;
        kept += 1;
    }
    let mut text = vec![LIST_HEAD.to_owned()];
    text.extend(lines[..kept].iter().cloned());
    if kept < lines.len() {
        text.push(rest(lines.len() - kept));
    }
    Some(text.join("\n"))
}

/// Decides a run's Skills at its first step.
pub fn for_turn(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
) -> anyhow::Result<TurnSkills> {
    let (_, _, skills) = runtime.list(workspace)?;
    let pinned: Vec<SkillInstanceId> = sent_pins(&skills).iter().map(|s| s.view.id).collect();
    Ok(TurnSkills { list: list_text(&names(&skills, &pinned), LIST_LIMIT_TOKENS), pinned })
}

/// The pinned Skills a run decided on, read again for one of its steps; one no longer usable is
/// left out.
pub fn pinned_texts(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
    pinned: &[SkillInstanceId],
) -> anyhow::Result<Vec<String>> {
    let (_, _, skills) = runtime.list(workspace)?;
    Ok(pinned
        .iter()
        .filter_map(|id| skills.iter().find(|skill| skill.view.id == *id))
        .filter_map(pinned_text)
        .collect())
}
