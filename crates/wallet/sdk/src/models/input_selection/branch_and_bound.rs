//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::Amount;

#[derive(Debug, Clone)]
pub struct KeyedInput<K> {
    key: K,
    value: u64,
}

impl<K> KeyedInput<K> {
    pub fn new(key: K, value: u64) -> Self {
        Self { key, value }
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn value(&self) -> u64 {
        self.value
    }
}

pub struct SelectionResult<'a, K> {
    total_value: Amount,
    selected_keys: Vec<&'a K>,
}

impl<'a, K> SelectionResult<'a, K> {
    pub fn total_value(&self) -> Amount {
        self.total_value
    }

    pub fn selected_keys(&self) -> &[&'a K] {
        &self.selected_keys
    }
}

struct State<'a, K> {
    index: usize,
    total: Amount,
    selected: Vec<&'a K>,
}

impl<'a, K> Clone for State<'a, K> {
    fn clone(&self) -> Self {
        State {
            index: self.index,
            total: self.total,
            selected: self.selected.clone(),
        }
    }
}

/// Find the smallest achievable sum >= target using an iterative branch-and-bound search.
/// This is simplified from the Bitcoin Core implementation because we do not take input weights and fees minimization
/// into account.
///
/// # Arguments
/// * `inputs` - Available inputs to select from
/// * `target` - Target amount to reach
/// * `max_inputs` - Maximum number of inputs that can be selected (e.g., 1000)
pub fn select<A: Into<Amount>, K: Clone>(
    inputs: &[KeyedInput<K>],
    target: A,
    max_inputs: usize,
) -> Option<SelectionResult<'_, K>> {
    // Sort descending to improve pruning efficiency
    // Collect references to avoid cloning keys/values unnecessarily
    let mut items = inputs.iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.value.cmp(&a.value));

    let mut best_sum: Option<Amount> = None;
    let mut best_keys = Vec::new();
    let target = target.into();

    // stack of states to explore
    let mut stack = vec![State {
        index: 0,
        total: Amount::zero(),
        selected: Vec::new(),
    }];

    while let Some(state) = stack.pop() {
        // --- Pruning conditions ---
        // Prune if we've exceeded the maximum number of inputs
        if state.selected.len() > max_inputs {
            continue;
        }

        if let Some(best) = best_sum {
            // already have a better or equal solution
            if best <= state.total {
                continue;
            }
        }

        // if total already meets or exceeds target → potential solution
        if target <= state.total {
            match best_sum {
                Some(best) if state.total < best => {
                    best_sum = Some(state.total);
                    best_keys = state.selected;
                    let change = state.total - target;
                    if change == 0 {
                        break; // optimal solution found
                    }
                },
                None => {
                    best_sum = Some(state.total);
                    best_keys = state.selected;
                },
                _ => {},
            }
            continue;
        }

        // if no more items, skip
        if state.index >= items.len() {
            continue;
        }

        // upper bound: even if we add everything left, can we reach target?
        let remaining_sum = items[state.index..]
            .iter()
            .map(|i| Amount::from(i.value))
            .sum::<Amount>();
        if target > state.total + remaining_sum {
            continue; // impossible to reach target → prune
        }

        // --- Branch 1: include current item (only if we haven't reached max inputs) ---
        if state.selected.len() < max_inputs {
            let mut with = state.clone();
            with.total += Amount::from(items[state.index].value);
            with.index += 1;
            with.selected.push(&items[state.index].key);
            stack.push(with);
        }

        // --- Branch 2: skip current item ---
        let mut without = state.clone();
        without.index += 1;
        stack.push(without);
    }

    best_sum.map(|total_value| SelectionResult {
        total_value,
        selected_keys: best_keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_finds_the_best_exact_fit() {
        let inputs = vec![
            KeyedInput { key: "A", value: 500 },
            KeyedInput { key: "B", value: 600 },
            KeyedInput { key: "C", value: 1000 },
        ];
        let target = 1100;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1100);
        assert_eq!(result.selected_keys.len(), 2);
    }

    #[test]
    fn test_empty_inputs() {
        let inputs: Vec<KeyedInput<&str>> = vec![];
        let target = 100;

        let result = select(&inputs, target, 1000);
        assert!(result.is_none());
    }

    #[test]
    fn test_insufficient_funds() {
        let inputs = vec![KeyedInput { key: "A", value: 100 }, KeyedInput { key: "B", value: 200 }];
        let target = 500;

        let result = select(&inputs, target, 1000);
        assert!(result.is_none());
    }

    #[test]
    fn test_single_input_exact_match() {
        let inputs = vec![KeyedInput { key: "A", value: 1000 }];
        let target = 1000;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1000);
        assert_eq!(result.selected_keys.len(), 1);
        assert_eq!(*result.selected_keys[0], "A");
    }

    #[test]
    fn test_single_input_overshoot() {
        let inputs = vec![KeyedInput { key: "A", value: 1500 }];
        let target = 1000;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1500);
        assert_eq!(result.selected_keys.len(), 1);
        assert_eq!(*result.selected_keys[0], "A");
    }

    #[test]
    fn test_multiple_solutions_finds_best() {
        let inputs = vec![
            KeyedInput { key: "A", value: 1000 },
            KeyedInput { key: "B", value: 500 },
            KeyedInput { key: "C", value: 600 },
        ];
        let target = 1100;

        let result = select(&inputs, target, 1000).unwrap();
        // Should prefer B+C (1100) over A alone (1000 is insufficient)
        assert_eq!(result.total_value, 1100);
        assert_eq!(result.selected_keys.len(), 2);
    }

    #[test]
    fn test_prefers_minimal_change() {
        let inputs = vec![
            KeyedInput { key: "A", value: 1000 },
            KeyedInput { key: "B", value: 500 },
            KeyedInput { key: "C", value: 600 },
            KeyedInput { key: "D", value: 200 },
        ];
        let target = 900;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1000);
    }

    #[test]
    fn test_large_set_performance() {
        let inputs: Vec<KeyedInput<usize>> = (1..=20)
            .map(|i| KeyedInput {
                key: i,
                value: i as u64 * 100,
            })
            .collect();
        let target = 1500;

        let start = std::time::Instant::now();
        let result = select(&inputs, target, 1000);
        let duration = start.elapsed();

        // Should complete quickly even with larger input sets
        assert!(duration.as_millis() < 1000);

        if let Some(selection) = result {
            assert!(selection.total_value >= target);
            assert!(!selection.selected_keys.is_empty());
        }
    }

    #[test]
    fn test_zero_target() {
        let inputs = vec![KeyedInput { key: "A", value: 100 }, KeyedInput { key: "B", value: 200 }];
        let target = 0;

        let result = select(&inputs, target, 1000).unwrap();
        // Should return empty selection since 0 target is already met
        assert_eq!(result.total_value, 0);
        assert_eq!(result.selected_keys.len(), 0);
    }

    #[test]
    fn test_duplicate_values() {
        let inputs = vec![
            KeyedInput { key: "A", value: 500 },
            KeyedInput { key: "B", value: 500 },
            KeyedInput { key: "C", value: 500 },
        ];
        let target = 1000;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1000);
        assert_eq!(result.selected_keys.len(), 2);
    }

    #[test]
    fn test_sorting_behavior() {
        let inputs = vec![
            KeyedInput {
                key: "small",
                value: 100,
            },
            KeyedInput {
                key: "large",
                value: 1000,
            },
            KeyedInput {
                key: "medium",
                value: 500,
            },
        ];
        let target = 500;

        let result = select(&inputs, target, 1000).unwrap();
        // Algorithm should find the medium value (500) as exact match
        assert_eq!(result.total_value, 500);
        assert_eq!(result.selected_keys.len(), 1);
        assert_eq!(*result.selected_keys[0], "medium");
    }

    #[test]
    fn test_greedy_vs_optimal() {
        let inputs = vec![
            KeyedInput { key: "A", value: 800 },
            KeyedInput { key: "B", value: 400 },
            KeyedInput { key: "C", value: 300 },
        ];
        let target = 700;

        let result = select(&inputs, target, 1000).unwrap();
        // Greedy would pick A (800), but optimal is B+C (700)
        assert_eq!(result.total_value, 700);
        assert_eq!(result.selected_keys.len(), 2);
    }

    #[test]
    fn test_max_inputs_limit_respected() {
        let inputs = vec![
            KeyedInput { key: "A", value: 200 },
            KeyedInput { key: "B", value: 150 },
            KeyedInput { key: "C", value: 50 },
            KeyedInput { key: "D", value: 50 },
            KeyedInput { key: "E", value: 400 },
        ];
        let target = 300;
        let max_inputs = 2;

        let result = select(&inputs, target, max_inputs).unwrap();
        // Should be able to reach 300 with A(200) + B(150) = 350
        assert_eq!(result.selected_keys.len(), 2);
        assert_eq!(result.total_value, 350);
    }

    #[test]
    fn test_max_inputs_limit_prevents_solution() {
        let inputs = vec![
            KeyedInput { key: "A", value: 100 },
            KeyedInput { key: "B", value: 100 },
            KeyedInput { key: "C", value: 100 },
        ];
        let target = 250;
        let max_inputs = 2; // Need 3 inputs to reach target, but limited to 2

        let result = select(&inputs, target, max_inputs);
        assert!(result.is_none());
    }

    #[test]
    fn test_max_inputs_limit_zero() {
        let inputs = vec![KeyedInput { key: "A", value: 100 }, KeyedInput { key: "B", value: 200 }];
        let target = 100;
        let max_inputs = 0;

        let result = select(&inputs, target, max_inputs);
        // Can't select any inputs with max_inputs = 0, so target > 0 should fail
        assert!(result.is_none());
    }

    #[test]
    fn test_max_inputs_limit_one() {
        let inputs = vec![
            KeyedInput { key: "A", value: 500 },
            KeyedInput { key: "B", value: 200 },
            KeyedInput { key: "C", value: 200 },
        ];
        let target = 400;
        let max_inputs = 1;

        let result = select(&inputs, target, max_inputs).unwrap();
        assert_eq!(result.selected_keys.len(), 1);
        assert!(result.total_value >= target);
        // Should select the largest input (500) since it's the only way to meet target with 1 input
        assert_eq!(*result.selected_keys[0], "A");
    }

    #[test]
    fn test_max_inputs_larger_than_available() {
        let inputs = vec![KeyedInput { key: "A", value: 100 }, KeyedInput { key: "B", value: 200 }];
        let target = 250;
        let max_inputs = 10; // More than available inputs

        let result = select(&inputs, target, max_inputs).unwrap();
        assert_eq!(result.selected_keys.len(), 2); // Uses all available inputs
        assert_eq!(result.total_value, 300);
    }
}
