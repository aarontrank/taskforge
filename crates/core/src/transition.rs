use crate::model::TaskStatus;

/// Whether a task may move directly from `from` to `to`.
///
/// Expressed as a match on the source state rather than as a lookup table, so the compiler
/// forces every status to be considered when a new one is added. Self-transitions are denied
/// everywhere: re-asserting the current status is a no-op, and treating it as a transition
/// would fire `task.status_changed` hooks for nothing.
///
/// Note that `Failed` is in the terminal set (it owns no live worker) yet still permits a
/// retry to `Running`. Terminal means "no worker holds this", not "immutable".
pub fn can_transition(from: TaskStatus, to: TaskStatus) -> bool {
    use TaskStatus::*;
    if from == to {
        return false;
    }
    match from {
        Open => matches!(to, Pending | Running | Cancelled),
        Pending => matches!(to, Running | Stuck | Cancelled),
        Running => matches!(to, InReview | Done | WaitingOnSchedule | Stuck | Failed),
        InReview => matches!(to, ChangesRequested | Merged | Stuck | Failed),
        ChangesRequested => matches!(to, Running | Stuck | Failed),
        Merged => matches!(to, Done | Failed),
        WaitingOnSchedule => matches!(to, Running | InReview | Stuck | Failed),
        Stuck => matches!(
            to,
            Running | Pending | InReview | WaitingOnSchedule | Cancelled | Failed
        ),
        Failed => matches!(to, Running | Cancelled),
        Done | Cancelled => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::model::TaskStatus::{self, *};
    use crate::transition::*;

    /// The specification, written as a flat list of legal (from, to) pairs.
    ///
    /// This list is deliberately a different shape from the `match` that implements it, so a
    /// discrepancy between spec and code is a test failure rather than two copies of the same
    /// mistake. Sourced from the orchestrate status board, not from the pre-Rust TypeScript
    /// model, which had only five states and therefore no opinion on most of these.
    const LEGAL: &[(TaskStatus, TaskStatus)] = &[
        // A created task is planned, dispatched, or abandoned before it ever runs.
        (Open, Pending),
        (Open, Running),
        (Open, Cancelled),
        // Planned-but-not-dispatched.
        (Pending, Running),
        (Pending, Stuck),
        (Pending, Cancelled),
        // Working. Review-gated work goes to InReview; ungated work completes outright.
        (Running, InReview),
        (Running, Done),
        (Running, WaitingOnSchedule),
        (Running, Stuck),
        (Running, Failed),
        // Awaiting human review.
        (InReview, ChangesRequested),
        (InReview, Merged),
        (InReview, Stuck),
        (InReview, Failed),
        // Feedback landed; the worker is addressing it.
        (ChangesRequested, Running),
        (ChangesRequested, Stuck),
        (ChangesRequested, Failed),
        // Merged is NOT terminal: it awaits human acceptance.
        (Merged, Done),
        (Merged, Failed),
        // Slow non-review step in flight.
        (WaitingOnSchedule, Running),
        (WaitingOnSchedule, InReview),
        (WaitingOnSchedule, Stuck),
        (WaitingOnSchedule, Failed),
        // Unsticking resumes wherever the work actually was.
        (Stuck, Running),
        (Stuck, Pending),
        (Stuck, InReview),
        (Stuck, WaitingOnSchedule),
        (Stuck, Cancelled),
        (Stuck, Failed),
        // Failed owns no worker, but may be retried or abandoned.
        (Failed, Running),
        (Failed, Cancelled),
    ];

    #[test]
    fn the_transition_table_matches_the_specification_exactly() {
        let mut wrong = Vec::new();
        for from in TaskStatus::ALL {
            for to in TaskStatus::ALL {
                let expected = LEGAL.contains(&(from, to));
                let actual = can_transition(from, to);
                if expected != actual {
                    wrong.push(format!(
                        "{from} -> {to}: spec says {expected}, code says {actual}"
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} mismatches:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn done_and_cancelled_admit_no_further_transition() {
        for to in TaskStatus::ALL {
            assert!(!can_transition(Done, to), "Done -> {to} must be denied");
            assert!(
                !can_transition(Cancelled, to),
                "Cancelled -> {to} must be denied"
            );
        }
    }

    #[test]
    fn merged_leads_to_done_but_is_not_itself_an_end_state() {
        assert!(
            can_transition(Merged, Done),
            "acceptance must be expressible"
        );
        assert!(!can_transition(Running, Merged), "merging skips review");
    }

    #[test]
    fn no_status_may_transition_to_itself() {
        for s in TaskStatus::ALL {
            assert!(
                !can_transition(s, s),
                "{s} -> {s} is a no-op, not a transition"
            );
        }
    }
}
