/// Context-type auto-classification (PROJECT_BRIEF.md §6).
///
/// A cheap keyword-scoring pass over the transcript that suggests a
/// context_type after transcription finishes. Deliberately heuristic — the
/// suggestion is surfaced in the UI for the user to confirm or override, so
/// a wrong guess costs one click. Returns None when the signal is too weak,
/// leaving the schema default ('meeting') in place.
const SAMPLE_CHARS: usize = 8000;

/// Phrases scored per context type. Multi-word phrases are worth more than
/// single words since they're much stronger signals.
const LECTURE_MARKERS: &[(&str, u32)] = &[
    ("professor", 3),
    ("lecture", 3),
    ("homework", 3),
    ("assignment", 2),
    ("syllabus", 3),
    ("midterm", 3),
    ("final exam", 3),
    ("exam", 2),
    ("quiz", 2),
    ("office hours", 3),
    ("textbook", 2),
    ("chapter", 1),
    ("problem set", 3),
    ("on the test", 3),
    ("next class", 2),
    ("semester", 2),
    ("course", 1),
];

const MEETING_MARKERS: &[(&str, u32)] = &[
    ("action item", 3),
    ("agenda", 3),
    ("standup", 3),
    ("stand-up", 3),
    ("sprint", 2),
    ("roadmap", 2),
    ("stakeholder", 3),
    ("deliverable", 2),
    ("follow up with the team", 3),
    ("meeting notes", 2),
    ("okr", 3),
    ("kpi", 2),
    ("quarterly", 2),
    ("deadline", 1),
    ("timeline", 1),
    ("sync", 1),
    ("blocked", 1),
    ("launch", 1),
];

const DISCUSSION_MARKERS: &[(&str, u32)] = &[
    ("brainstorm", 3),
    ("study group", 3),
    ("what does everyone think", 3),
    ("what do you all think", 3),
    ("let's go around", 2),
    ("bounce ideas", 3),
    ("thoughts on", 1),
    ("devil's advocate", 2),
    ("compare notes", 2),
    ("work through", 1),
    ("problem together", 2),
];

const COFFEE_CHAT_MARKERS: &[(&str, u32)] = &[
    ("nice to meet you", 3),
    ("great to meet you", 3),
    ("how have you been", 3),
    ("catch up", 3),
    ("grab coffee", 3),
    ("grab lunch", 3),
    ("keep in touch", 3),
    ("your career", 2),
    ("my career", 2),
    ("how did you get into", 3),
    ("stay in touch", 3),
    ("connect on linkedin", 3),
    ("happy to intro", 2),
    ("how's the family", 3),
    ("congrats", 1),
];

fn score(haystack: &str, markers: &[(&str, u32)]) -> u32 {
    markers
        .iter()
        .map(|(phrase, weight)| {
            let hits = haystack.matches(phrase).count().min(3) as u32; // cap repeats
            hits * weight
        })
        .sum()
}

/// Suggest a context_type from transcript text, or None when unsure.
pub fn suggest_context_type(transcript_text: &str) -> Option<&'static str> {
    let sample: String = transcript_text
        .chars()
        .take(SAMPLE_CHARS)
        .collect::<String>()
        .to_lowercase();

    if sample.trim().len() < 80 {
        return None; // too little signal to classify
    }

    let scores = [
        ("lecture", score(&sample, LECTURE_MARKERS)),
        ("meeting", score(&sample, MEETING_MARKERS)),
        ("discussion", score(&sample, DISCUSSION_MARKERS)),
        ("coffee_chat", score(&sample, COFFEE_CHAT_MARKERS)),
    ];

    let (best, best_score) = *scores.iter().max_by_key(|(_, s)| *s)?;
    let runner_up = scores
        .iter()
        .filter(|(name, _)| *name != best)
        .map(|(_, s)| *s)
        .max()
        .unwrap_or(0);

    // Require real signal and a clear margin over the runner-up
    if best_score >= 5 && best_score >= runner_up + 3 {
        Some(best)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_lecture() {
        let text = "Welcome everyone, today's lecture covers dynamic programming. \
                    Remember the homework is due Friday, and this material will be on the midterm. \
                    The professor mentioned chapter 12 of the textbook covers this in depth. \
                    Come to office hours if you have questions about the problem set.";
        assert_eq!(suggest_context_type(text), Some("lecture"));
    }

    #[test]
    fn classifies_meeting() {
        let text = "Let's run through the agenda. First action item from last sprint: \
                    the roadmap review with stakeholders. The deliverable deadline moved. \
                    Second action item: update the OKR dashboard before the quarterly review.";
        assert_eq!(suggest_context_type(text), Some("meeting"));
    }

    #[test]
    fn classifies_coffee_chat() {
        let text = "Hey, so nice to meet you! I've been meaning to catch up for ages. \
                    How have you been since the move? We should definitely keep in touch — \
                    let's connect on LinkedIn and grab coffee again next month.";
        assert_eq!(suggest_context_type(text), Some("coffee_chat"));
    }

    #[test]
    fn ambiguous_text_returns_none() {
        let text = "So yeah, that's basically what happened over the weekend. \
                    It rained most of Saturday so we stayed in and watched movies, \
                    then Sunday we walked around the park for a couple of hours.";
        assert_eq!(suggest_context_type(text), None);
    }

    #[test]
    fn short_text_returns_none() {
        assert_eq!(suggest_context_type("brief hello"), None);
    }
}
