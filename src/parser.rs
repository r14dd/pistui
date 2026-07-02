use std::collections::{HashMap, HashSet};

use crate::model::{ModelStats, Observation, Score, SpanKind, SpanNode, SpanStatus, TraceStats};

pub fn build_tree(observations: &[Observation], scores: &[Score]) -> (Vec<SpanNode>, TraceStats) {
    let obs_ids: HashSet<&str> = observations.iter().map(|o| o.id.as_str()).collect();

    let mut children_map: HashMap<Option<&str>, Vec<&Observation>> = HashMap::new();
    for obs in observations {
        children_map
            .entry(obs.parent_observation_id.as_deref())
            .or_default()
            .push(obs);
    }

    for group in children_map.values_mut() {
        group.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    }

    let mut score_map: HashMap<&str, Vec<&Score>> = HashMap::new();
    for score in scores {
        if let Some(obs_id) = score.observation_id.as_deref() {
            score_map.entry(obs_id).or_default().push(score);
        }
    }

    let mut roots = build_children(None, &children_map, &score_map);

    // Re-root orphans: observations whose declared parent doesn't exist in this trace
    let mut orphan_count = 0;
    for parent_id in children_map.keys().copied().flatten() {
        if !obs_ids.contains(parent_id) {
            let orphans = build_children(Some(parent_id), &children_map, &score_map);
            orphan_count += orphans.len();
            roots.extend(orphans);
        }
    }

    roots.sort_by(|a, b| a.start_time.cmp(&b.start_time));

    let mut stats = compute_stats(&roots, observations);
    stats.orphan_count = orphan_count;
    (roots, stats)
}

fn build_children<'a>(
    parent_id: Option<&str>,
    children_map: &HashMap<Option<&'a str>, Vec<&'a Observation>>,
    score_map: &HashMap<&str, Vec<&Score>>,
) -> Vec<SpanNode> {
    let Some(children) = children_map.get(&parent_id) else {
        return Vec::new();
    };

    children
        .iter()
        .map(|obs| {
            let children = build_children(Some(&obs.id), children_map, score_map);
            let duration_ms = duration_between(&obs.start_time, obs.end_time.as_deref());
            let ttft_ms = if obs.kind == SpanKind::Generation {
                obs.completion_start_time
                    .as_deref()
                    .and_then(|ct| duration_between(&obs.start_time, Some(ct)))
            } else {
                None
            };

            let status = if is_error(obs) {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            };

            let (tokens_in, tokens_out, tokens_total) = token_counts(obs);

            let scores = score_map
                .get(obs.id.as_str())
                .map(|v| v.iter().map(|s| (*s).clone()).collect())
                .unwrap_or_default();

            SpanNode {
                id: obs.id.clone(),
                name: obs.name.clone().unwrap_or_else(|| obs.kind.to_string()),
                kind: obs.kind,
                start_time: obs.start_time.clone(),
                end_time: obs.end_time.clone(),
                duration_ms,
                ttft_ms,
                status,
                status_message: obs.status_message.clone(),
                model: obs.model.clone(),
                input: obs.input.clone(),
                output: obs.output.clone(),
                metadata: obs.metadata.clone(),
                tokens_in,
                tokens_out,
                tokens_total,
                cost: obs.calculated_total_cost,
                scores,
                children,
            }
        })
        .collect()
}

pub fn is_error(obs: &Observation) -> bool {
    obs.level.as_deref() == Some("ERROR")
}

fn token_counts(obs: &Observation) -> (Option<u64>, Option<u64>, Option<u64>) {
    match &obs.usage {
        None => (None, None, None),
        Some(u) => {
            let tin = u.input;
            let tout = u.output;
            let ttotal = u.total;
            // Prefer explicit in/out; fall back to total when both are absent
            let effective_total = match (tin, tout, ttotal) {
                (None, None, Some(t)) => Some(t),
                _ => ttotal,
            };
            (tin, tout, effective_total)
        }
    }
}

fn duration_between(start: &str, end: Option<&str>) -> Option<f64> {
    let end = end?;
    let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(end).ok()?;
    let ms = (end - start).num_milliseconds() as f64;
    Some(ms.max(0.0))
}

fn compute_stats(roots: &[SpanNode], observations: &[Observation]) -> TraceStats {
    let mut stats = TraceStats {
        total_duration_ms: None,
        total_tokens_in: 0,
        total_tokens_out: 0,
        total_cost: 0.0,
        span_count: 0,
        generation_count: 0,
        event_count: 0,
        error_count: 0,
        deepest_nesting: 0,
        orphan_count: 0,
        truncated: false,
        model_breakdown: Vec::new(),
    };

    let mut model_map: HashMap<String, (usize, u64, u64, f64)> = HashMap::new();

    for obs in observations {
        match obs.kind {
            SpanKind::Span => stats.span_count += 1,
            SpanKind::Generation => stats.generation_count += 1,
            SpanKind::Event => stats.event_count += 1,
        }
        if is_error(obs) {
            stats.error_count += 1;
        }

        let (tin, tout, ttotal) = token_counts(obs);
        // Use explicit in/out for totals; fall back to total only when both are absent
        let counted_in = tin.or_else(|| if tout.is_none() { ttotal } else { None });
        let counted_out = tout.unwrap_or(0);
        stats.total_tokens_in = stats
            .total_tokens_in
            .saturating_add(counted_in.unwrap_or(0));
        stats.total_tokens_out = stats.total_tokens_out.saturating_add(counted_out);

        let cost = obs.calculated_total_cost.unwrap_or(0.0);
        stats.total_cost += cost;

        if let Some(model) = &obs.model {
            let entry = model_map.entry(model.clone()).or_default();
            entry.0 += 1;
            entry.1 = entry.1.saturating_add(counted_in.unwrap_or(0));
            entry.2 = entry.2.saturating_add(counted_out);
            entry.3 += cost;
        }
    }

    stats.model_breakdown = model_map
        .into_iter()
        .map(|(model, (count, tokens_in, tokens_out, cost))| ModelStats {
            model,
            count,
            tokens_in,
            tokens_out,
            cost,
        })
        .collect();
    stats.model_breakdown.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
    });

    if let (Some(earliest), Some(latest)) = (earliest_start(observations), latest_end(observations))
    {
        stats.total_duration_ms = duration_between(&earliest, Some(&latest));
    }

    stats.deepest_nesting = max_depth(roots, 0);
    stats
}

fn earliest_start(observations: &[Observation]) -> Option<String> {
    observations.iter().map(|o| &o.start_time).min().cloned()
}

fn latest_end(observations: &[Observation]) -> Option<String> {
    observations
        .iter()
        .filter_map(|o| o.end_time.as_ref())
        .max()
        .cloned()
}

fn max_depth(nodes: &[SpanNode], current: usize) -> usize {
    if nodes.is_empty() {
        return current;
    }
    nodes
        .iter()
        .map(|n| max_depth(&n.children, current + 1))
        .max()
        .unwrap_or(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Usage;

    fn obs(
        id: &str,
        parent: Option<&str>,
        kind: SpanKind,
        start: &str,
        end: Option<&str>,
    ) -> Observation {
        Observation {
            id: id.to_string(),
            trace_id: "trace-1".to_string(),
            kind,
            name: Some(id.to_string()),
            start_time: start.to_string(),
            end_time: end.map(String::from),
            completion_start_time: None,
            model: None,
            input: None,
            output: None,
            metadata: None,
            level: None,
            status_message: None,
            parent_observation_id: parent.map(String::from),
            usage: None,
            calculated_total_cost: None,
        }
    }

    #[test]
    fn empty_observations_produce_empty_tree() {
        let (tree, stats) = build_tree(&[], &[]);
        assert!(tree.is_empty());
        assert_eq!(stats.span_count, 0);
    }

    #[test]
    fn single_root_span() {
        let observations = vec![obs(
            "root",
            None,
            SpanKind::Span,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        )];
        let (tree, stats) = build_tree(&observations, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "root");
        assert_eq!(tree[0].duration_ms, Some(1000.0));
        assert!(tree[0].children.is_empty());
        assert_eq!(stats.span_count, 1);
        assert_eq!(stats.deepest_nesting, 1);
    }

    #[test]
    fn nested_tree_structure() {
        let observations = vec![
            obs(
                "root",
                None,
                SpanKind::Span,
                "2026-06-10T14:00:00.000Z",
                Some("2026-06-10T14:00:03.000Z"),
            ),
            obs(
                "child-a",
                Some("root"),
                SpanKind::Generation,
                "2026-06-10T14:00:00.500Z",
                Some("2026-06-10T14:00:01.500Z"),
            ),
            obs(
                "child-b",
                Some("root"),
                SpanKind::Event,
                "2026-06-10T14:00:02.000Z",
                None,
            ),
            obs(
                "grandchild",
                Some("child-a"),
                SpanKind::Span,
                "2026-06-10T14:00:00.600Z",
                Some("2026-06-10T14:00:01.000Z"),
            ),
        ];
        let (tree, stats) = build_tree(&observations, &[]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].name, "child-a");
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].name, "grandchild");
        assert_eq!(tree[0].children[1].name, "child-b");

        assert_eq!(stats.span_count, 2);
        assert_eq!(stats.generation_count, 1);
        assert_eq!(stats.event_count, 1);
        assert_eq!(stats.deepest_nesting, 3);
    }

    #[test]
    fn orphaned_observations_become_roots() {
        let observations = vec![
            obs(
                "orphan",
                Some("nonexistent"),
                SpanKind::Span,
                "2026-06-10T14:00:00.000Z",
                Some("2026-06-10T14:00:01.000Z"),
            ),
            obs(
                "root",
                None,
                SpanKind::Span,
                "2026-06-10T14:00:00.000Z",
                Some("2026-06-10T14:00:02.000Z"),
            ),
        ];
        let (tree, stats) = build_tree(&observations, &[]);
        // Orphan is re-rooted rather than dropped
        assert_eq!(tree.len(), 2);
        assert!(tree.iter().any(|n| n.name == "root"));
        assert!(tree.iter().any(|n| n.name == "orphan"));
        assert_eq!(stats.orphan_count, 1);
    }

    #[test]
    fn error_status_requires_level_error() {
        let mut error_obs = obs(
            "fail",
            None,
            SpanKind::Span,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        error_obs.level = Some("ERROR".into());
        error_obs.status_message = Some("boom".into());

        let (tree, stats) = build_tree(&[error_obs], &[]);
        assert_eq!(tree[0].status, SpanStatus::Error);
        assert_eq!(stats.error_count, 1);
    }

    #[test]
    fn status_message_without_error_level_is_not_an_error() {
        let mut ok_obs = obs(
            "note",
            None,
            SpanKind::Span,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        // status_message without level == ERROR is just informational
        ok_obs.status_message = Some("cache miss".into());

        let (tree, stats) = build_tree(&[ok_obs], &[]);
        assert_eq!(tree[0].status, SpanStatus::Ok);
        assert_eq!(stats.error_count, 0);
    }

    #[test]
    fn token_and_cost_aggregation() {
        let mut gen = obs(
            "llm",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        gen.usage = Some(Usage {
            input: Some(100),
            output: Some(50),
            total: Some(150),
            unit: Some("TOKENS".into()),
        });
        gen.calculated_total_cost = Some(0.003);

        let (_, stats) = build_tree(&[gen], &[]);
        assert_eq!(stats.total_tokens_in, 100);
        assert_eq!(stats.total_tokens_out, 50);
        assert!((stats.total_cost - 0.003).abs() < f64::EPSILON);
    }

    #[test]
    fn token_fallback_to_total_when_in_out_absent() {
        let mut gen = obs(
            "llm",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        gen.usage = Some(Usage {
            input: None,
            output: None,
            total: Some(200),
            unit: None,
        });

        let (tree, stats) = build_tree(&[gen], &[]);
        // When only total is present, it falls through to total_tokens_in as the best approximation
        assert_eq!(tree[0].tokens_total, Some(200));
        assert_eq!(stats.total_tokens_in, 200);
        assert_eq!(stats.total_tokens_out, 0);
    }

    #[test]
    fn model_breakdown_aggregates_per_model() {
        let mut gen1 = obs(
            "call-1",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        gen1.model = Some("gpt-4o".into());
        gen1.usage = Some(Usage {
            input: Some(100),
            output: Some(50),
            total: Some(150),
            unit: None,
        });
        gen1.calculated_total_cost = Some(0.01);

        let mut gen2 = obs(
            "call-2",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:01.000Z",
            Some("2026-06-10T14:00:02.000Z"),
        );
        gen2.model = Some("gpt-4o".into());
        gen2.usage = Some(Usage {
            input: Some(200),
            output: Some(100),
            total: Some(300),
            unit: None,
        });
        gen2.calculated_total_cost = Some(0.02);

        let mut gen3 = obs(
            "call-3",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:02.000Z",
            Some("2026-06-10T14:00:03.000Z"),
        );
        gen3.model = Some("claude-3-haiku".into());
        gen3.usage = Some(Usage {
            input: Some(50),
            output: Some(25),
            total: Some(75),
            unit: None,
        });
        gen3.calculated_total_cost = Some(0.001);

        let (_, stats) = build_tree(&[gen1, gen2, gen3], &[]);
        assert_eq!(stats.model_breakdown.len(), 2);

        let gpt = stats
            .model_breakdown
            .iter()
            .find(|m| m.model == "gpt-4o")
            .unwrap();
        assert_eq!(gpt.count, 2);
        assert_eq!(gpt.tokens_in, 300);
        assert_eq!(gpt.tokens_out, 150);
        assert!((gpt.cost - 0.03).abs() < f64::EPSILON);

        let claude = stats
            .model_breakdown
            .iter()
            .find(|m| m.model == "claude-3-haiku")
            .unwrap();
        assert_eq!(claude.count, 1);
        assert_eq!(claude.tokens_in, 50);
    }

    #[test]
    fn model_breakdown_breaks_cost_ties_alphabetically() {
        let mut gen1 = obs(
            "a",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        gen1.model = Some("zebra".into());
        gen1.calculated_total_cost = Some(0.01);

        let mut gen2 = obs(
            "b",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:01.000Z",
            Some("2026-06-10T14:00:02.000Z"),
        );
        gen2.model = Some("alpha".into());
        gen2.calculated_total_cost = Some(0.01);

        let (_, stats) = build_tree(&[gen1, gen2], &[]);
        assert_eq!(stats.model_breakdown[0].model, "alpha");
        assert_eq!(stats.model_breakdown[1].model, "zebra");
    }

    #[test]
    fn children_sorted_by_start_time() {
        let observations = vec![
            obs(
                "root",
                None,
                SpanKind::Span,
                "2026-06-10T14:00:00.000Z",
                Some("2026-06-10T14:00:05.000Z"),
            ),
            obs(
                "late",
                Some("root"),
                SpanKind::Span,
                "2026-06-10T14:00:03.000Z",
                Some("2026-06-10T14:00:04.000Z"),
            ),
            obs(
                "early",
                Some("root"),
                SpanKind::Span,
                "2026-06-10T14:00:01.000Z",
                Some("2026-06-10T14:00:02.000Z"),
            ),
        ];
        let (tree, _) = build_tree(&observations, &[]);
        assert_eq!(tree[0].children[0].name, "early");
        assert_eq!(tree[0].children[1].name, "late");
    }

    #[test]
    fn scores_attached_to_matching_span() {
        use crate::model::Score;
        let observations = vec![obs(
            "obs-1",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        )];
        let scores = vec![Score {
            id: "s-1".into(),
            trace_id: "trace-1".into(),
            name: "accuracy".into(),
            value: Some(0.9),
            string_value: None,
            comment: None,
            observation_id: Some("obs-1".into()),
            source: None,
            data_type: Some("NUMERIC".into()),
        }];
        let (tree, _) = build_tree(&observations, &scores);
        assert_eq!(tree[0].scores.len(), 1);
        assert_eq!(tree[0].scores[0].name, "accuracy");
    }

    #[test]
    fn ttft_computed_for_generation_with_completion_start_time() {
        let mut gen = obs(
            "llm",
            None,
            SpanKind::Generation,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:02.000Z"),
        );
        gen.completion_start_time = Some("2026-06-10T14:00:00.500Z".into());

        let (tree, _) = build_tree(&[gen], &[]);
        assert_eq!(tree[0].ttft_ms, Some(500.0));
    }
}
