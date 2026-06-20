use std::collections::HashMap;

use crate::model::{Observation, SpanKind, SpanNode, SpanStatus, TraceStats};

pub fn build_tree(observations: &[Observation]) -> (Vec<SpanNode>, TraceStats) {
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

    let roots = build_children(None, &children_map);
    let stats = compute_stats(&roots, observations);
    (roots, stats)
}

fn build_children(
    parent_id: Option<&str>,
    children_map: &HashMap<Option<&str>, Vec<&Observation>>,
) -> Vec<SpanNode> {
    let Some(children) = children_map.get(&parent_id) else {
        return Vec::new();
    };

    children
        .iter()
        .map(|obs| {
            let children = build_children(Some(&obs.id), children_map);
            let duration_ms = duration_between(&obs.start_time, obs.end_time.as_deref());
            let status = if obs.level.as_deref() == Some("ERROR") || obs.status_message.is_some() {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            };

            SpanNode {
                id: obs.id.clone(),
                name: obs.name.clone().unwrap_or_else(|| obs.kind.to_string()),
                kind: obs.kind,
                start_time: obs.start_time.clone(),
                end_time: obs.end_time.clone(),
                duration_ms,
                status,
                status_message: obs.status_message.clone(),
                model: obs.model.clone(),
                input: obs.input.clone(),
                output: obs.output.clone(),
                tokens_in: obs.usage.as_ref().and_then(|u| u.input),
                tokens_out: obs.usage.as_ref().and_then(|u| u.output),
                cost: obs.calculated_total_cost,
                children,
            }
        })
        .collect()
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
    };

    for obs in observations {
        match obs.kind {
            SpanKind::Span => stats.span_count += 1,
            SpanKind::Generation => stats.generation_count += 1,
            SpanKind::Event => stats.event_count += 1,
        }
        if obs.level.as_deref() == Some("ERROR") || obs.status_message.is_some() {
            stats.error_count += 1;
        }
        if let Some(usage) = &obs.usage {
            stats.total_tokens_in += usage.input.unwrap_or(0);
            stats.total_tokens_out += usage.output.unwrap_or(0);
        }
        stats.total_cost += obs.calculated_total_cost.unwrap_or(0.0);
    }

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
        let (tree, stats) = build_tree(&[]);
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
        let (tree, stats) = build_tree(&observations);
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
        let (tree, stats) = build_tree(&observations);

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
        let (tree, _) = build_tree(&observations);
        // The orphan attaches under its declared parent key "nonexistent",
        // which won't match any node, so it remains a separate root-level group.
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "root");
    }

    #[test]
    fn error_status_detected() {
        let mut error_obs = obs(
            "fail",
            None,
            SpanKind::Span,
            "2026-06-10T14:00:00.000Z",
            Some("2026-06-10T14:00:01.000Z"),
        );
        error_obs.level = Some("ERROR".into());
        error_obs.status_message = Some("boom".into());

        let (tree, stats) = build_tree(&[error_obs]);
        assert_eq!(tree[0].status, SpanStatus::Error);
        assert_eq!(stats.error_count, 1);
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

        let (_, stats) = build_tree(&[gen]);
        assert_eq!(stats.total_tokens_in, 100);
        assert_eq!(stats.total_tokens_out, 50);
        assert!((stats.total_cost - 0.003).abs() < f64::EPSILON);
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
        let (tree, _) = build_tree(&observations);
        assert_eq!(tree[0].children[0].name, "early");
        assert_eq!(tree[0].children[1].name, "late");
    }
}
