//! The prompt that hands review comments to an agent (`reviewPrompt.ts`).

use indexmap::IndexMap;

use super::{Freshness, Resolution, ReviewComment};

/// A markdown prompt for `comments`, whatever their addressed state (re-prompting one
/// comment). Grouped by file in first-seen order; empty for no comments.
pub fn build_prompt_for_comments(comments: &[&ReviewComment]) -> String {
    if comments.is_empty() {
        return String::new();
    }

    let mut grouped: IndexMap<&str, Vec<&ReviewComment>> = IndexMap::new();
    for comment in comments {
        grouped
            .entry(comment.file_path.as_str())
            .or_default()
            .push(comment);
    }

    let mut lines = vec![
        "Please address the following review comments:".to_owned(),
        String::new(),
    ];
    for (file_path, file_comments) in grouped {
        lines.push(format!("## {file_path}"));
        for comment in file_comments {
            let outdated = match comment.freshness {
                Freshness::Outdated => " [OUTDATED]",
                Freshness::Current => "",
            };
            lines.push(format!(
                "- Line {} ({}): \"{}\"{outdated}",
                comment.line_number,
                comment.side.as_str(),
                comment.text
            ));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

/// The prompt for the comments not yet addressed.
pub fn generate_review_prompt(comments: &[ReviewComment]) -> String {
    let open: Vec<&ReviewComment> = comments
        .iter()
        .filter(|comment| comment.resolution == Resolution::Open)
        .collect();
    build_prompt_for_comments(&open)
}

#[cfg(test)]
mod tests {
    //! Port of `utils/reviewPrompt.test.ts`, case by case.

    use super::*;
    use crate::review::{CommitAnchor, LineAnchor, Side};

    fn make_comment(id: &str, file_path: &str, line_number: u32, text: &str) -> ReviewComment {
        ReviewComment {
            id: id.to_owned(),
            file_path: file_path.to_owned(),
            line_number,
            text: text.to_owned(),
            commit: CommitAnchor::Head("abc123".to_owned()),
            line: LineAnchor::Untracked,
            created_at: 1,
            freshness: Freshness::Current,
            resolution: Resolution::Open,
            side: Side::Modified,
        }
    }

    fn default_comment() -> ReviewComment {
        make_comment("c1", "src/foo.ts", 10, "fix this")
    }

    fn addressed(mut comment: ReviewComment) -> ReviewComment {
        comment.resolution = Resolution::Addressed;
        comment
    }

    fn outdated(mut comment: ReviewComment) -> ReviewComment {
        comment.freshness = Freshness::Outdated;
        comment
    }

    mod generate_review_prompt {
        use super::*;

        #[test]
        fn returns_empty_string_for_empty_array() {
            assert_eq!(generate_review_prompt(&[]), "");
        }

        #[test]
        fn returns_empty_string_when_all_comments_are_addressed() {
            let comments = [
                addressed(default_comment()),
                addressed(make_comment("c2", "src/foo.ts", 10, "fix this")),
            ];
            assert_eq!(generate_review_prompt(&comments), "");
        }

        #[test]
        fn formats_a_single_unaddressed_comment() {
            let result = generate_review_prompt(&[default_comment()]);
            assert!(result.contains("Please address the following review comments:"));
            assert!(result.contains("## src/foo.ts"));
            assert!(result.contains("- Line 10 (modified): \"fix this\""));
            assert_eq!(
                result,
                "Please address the following review comments:\n\n## src/foo.ts\n- Line 10 (modified): \"fix this\"\n"
            );
        }

        #[test]
        fn groups_comments_by_file_path() {
            let comments = [
                make_comment("c1", "a.ts", 1, "one"),
                make_comment("c2", "b.ts", 2, "two"),
                make_comment("c3", "a.ts", 5, "three"),
            ];
            let result = generate_review_prompt(&comments);
            assert!(result.contains("## a.ts"));
            assert!(result.contains("## b.ts"));
            let a_section = result.split("## b.ts").next().unwrap();
            assert!(a_section.contains("\"one\""));
            assert!(a_section.contains("\"three\""));
        }

        #[test]
        fn adds_outdated_suffix_to_outdated_comments() {
            assert!(generate_review_prompt(&[outdated(default_comment())]).contains("[OUTDATED]"));
        }

        #[test]
        fn does_not_add_outdated_to_non_outdated_comments() {
            assert!(!generate_review_prompt(&[default_comment()]).contains("[OUTDATED]"));
        }

        #[test]
        fn filters_out_addressed_comments_and_formats_unaddressed_ones() {
            let comments = [
                addressed(make_comment("c1", "src/foo.ts", 10, "done")),
                make_comment("c2", "src/foo.ts", 10, "pending"),
                outdated(make_comment("c3", "src/foo.ts", 10, "old")),
            ];
            let result = generate_review_prompt(&comments);
            assert!(!result.contains("\"done\""));
            assert!(result.contains("\"pending\""));
            assert!(result.contains("\"old\" [OUTDATED]"));
        }
    }

    mod build_prompt_for_comments {
        use super::*;

        #[test]
        fn returns_empty_string_for_empty_array() {
            assert_eq!(build_prompt_for_comments(&[]), "");
        }

        #[test]
        fn formats_addressed_comments_without_filtering_them_out() {
            let comment = addressed(make_comment("c1", "src/foo.ts", 10, "still prompt me"));
            let result = build_prompt_for_comments(&[&comment]);
            assert!(result.contains("## src/foo.ts"));
            assert!(result.contains("\"still prompt me\""));
        }

        #[test]
        fn formats_a_single_comment_for_re_prompting() {
            let comment = make_comment("c1", "src/foo.ts", 42, "tweak");
            assert!(
                build_prompt_for_comments(&[&comment]).contains("- Line 42 (modified): \"tweak\"")
            );
        }
    }
}
