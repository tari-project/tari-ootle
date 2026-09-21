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

/// The number of search states explored before the search settles for the best selection it has found so far.
/// Bitcoin Core bounds its own branch-and-bound with the same number.
///
/// The search is exponential in the number of inputs whenever no subset totals the target exactly — a set of
/// equal-valued outputs is the everyday shape of that — so it must be bounded for selection time to be
/// independent of how an account's balance is split.
const MAX_SEARCH_STATES: usize = 100_000;

/// Find the smallest achievable sum >= target using an iterative branch-and-bound search.
/// This is simplified from the Bitcoin Core implementation because we do not take input weights and fees minimization
/// into account.
///
/// The search explores at most [`MAX_SEARCH_STATES`] states; beyond that it returns the best selection found, which
/// reaches the target but may leave more change than the smallest one would. `None` means the target is unreachable —
/// either the inputs do not total it, or no `max_inputs` of them do.
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
    select_bounded(inputs, target, max_inputs, MAX_SEARCH_STATES)
}

fn select_bounded<A: Into<Amount>, K: Clone>(
    inputs: &[KeyedInput<K>],
    target: A,
    max_inputs: usize,
    max_states: usize,
) -> Option<SelectionResult<'_, K>> {
    // Sort descending to improve pruning efficiency
    // Collect references to avoid cloning keys/values unnecessarily
    let mut items = inputs.iter().collect::<Vec<_>>();
    items.sort_by_key(|b| std::cmp::Reverse(b.value));

    let mut best_sum: Option<Amount> = None;
    let mut best_keys = Vec::new();
    let target = target.into();

    // No selection of at most `max_inputs` inputs can total more than the `max_inputs` largest, so this decides
    // reachability outright: a `None` return always means the target is out of reach, never that the search gave up.
    let max_achievable = items
        .iter()
        .take(max_inputs)
        .map(|i| Amount::from(i.value))
        .sum::<Amount>();
    if max_achievable < target {
        return None;
    }

    // `remaining_sums[i]` totals `items[i..]`, so that a state's upper bound costs one lookup and the search's cost
    // stays proportional to the number of states it explores.
    let mut remaining_sums = vec![Amount::zero(); items.len() + 1];
    for (i, item) in items.iter().enumerate().rev() {
        remaining_sums[i] = remaining_sums[i + 1] + Amount::from(item.value);
    }

    // stack of states to explore
    let mut stack = vec![State {
        index: 0,
        total: Amount::zero(),
        selected: Vec::new(),
    }];

    let mut states_explored = 0usize;

    while let Some(state) = stack.pop() {
        states_explored += 1;
        if states_explored > max_states {
            break;
        }

        // --- Pruning conditions ---
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
        if target > state.total + remaining_sums[state.index] {
            continue; // impossible to reach target → prune
        }

        // --- Branch 2: skip current item ---
        let mut without = state.clone();
        without.index += 1;
        stack.push(without);

        // --- Branch 1: include current item (only if we haven't reached max inputs) ---
        // Pushed last so that it is explored first: the search then descends largest-first, reaching the target
        // within `max_inputs` states and giving every state after it a total to be pruned against.
        if state.selected.len() < max_inputs {
            let mut with = state.clone();
            with.total += Amount::from(items[state.index].value);
            with.index += 1;
            with.selected.push(&items[state.index].key);
            stack.push(with);
        }
    }

    match best_sum {
        Some(total_value) => Some(SelectionResult {
            total_value,
            selected_keys: best_keys,
        }),
        // The search ran out of states before it reached any solution. The largest-first selection is one whenever
        // the target is reachable at all, which the check above has established it is.
        None => take_largest_first(&items, target, max_inputs),
    }
}

/// The target reached with the fewest of the largest inputs, or `None` if `max_inputs` of them do not reach it.
/// `items` must be sorted by descending value.
fn take_largest_first<'a, K>(
    items: &[&'a KeyedInput<K>],
    target: Amount,
    max_inputs: usize,
) -> Option<SelectionResult<'a, K>> {
    let mut total_value = Amount::zero();
    let mut selected_keys = Vec::new();
    for item in items.iter().take(max_inputs) {
        if target <= total_value {
            break;
        }
        total_value += Amount::from(item.value);
        selected_keys.push(&item.key);
    }

    if total_value < target {
        return None;
    }

    Some(SelectionResult {
        total_value,
        selected_keys,
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
        let target = 1100u64;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1100);
        assert_eq!(result.selected_keys.len(), 2);
    }

    #[test]
    fn test_empty_inputs() {
        let inputs: Vec<KeyedInput<&str>> = vec![];
        let target = 100u64;

        let result = select(&inputs, target, 1000);
        assert!(result.is_none());
    }

    #[test]
    fn test_insufficient_funds() {
        let inputs = vec![KeyedInput { key: "A", value: 100 }, KeyedInput { key: "B", value: 200 }];
        let target = 500u64;

        let result = select(&inputs, target, 1000);
        assert!(result.is_none());
    }

    #[test]
    fn test_single_input_exact_match() {
        let inputs = vec![KeyedInput { key: "A", value: 1000 }];
        let target = 1000u64;

        let result = select(&inputs, target, 1000).unwrap();
        assert_eq!(result.total_value, 1000);
        assert_eq!(result.selected_keys.len(), 1);
        assert_eq!(*result.selected_keys[0], "A");
    }

    #[test]
    fn test_single_input_overshoot() {
        let inputs = vec![KeyedInput { key: "A", value: 1500 }];
        let target = 1000u64;

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
        let target = 1100u64;

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
        let target = 900u64;

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
        let target = 1500u64;

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
        let target = 0u64;

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
        let target = 1000u64;

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
        let target = 500u64;

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
        let target = 700u64;

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
        let target = 300u64;
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
        let target = 250u64;
        let max_inputs = 2; // Need 3 inputs to reach target, but limited to 2

        let result = select(&inputs, target, max_inputs);
        assert!(result.is_none());
    }

    #[test]
    fn test_max_inputs_limit_zero() {
        let inputs = vec![KeyedInput { key: "A", value: 100 }, KeyedInput { key: "B", value: 200 }];
        let target = 100u64;
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
        let target = 400u64;
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
        let target = 250u64;
        let max_inputs = 10; // More than available inputs

        let result = select(&inputs, target, max_inputs).unwrap();
        assert_eq!(result.selected_keys.len(), 2); // Uses all available inputs
        assert_eq!(result.total_value, 300);
    }

    #[test]
    fn it_terminates_when_no_subset_totals_the_target() {
        // Equal-valued inputs with a target that no subset of them totals exactly: the shape that has no
        // exact fit to stop the search early, and the one a wallet paid in equal amounts ends up in.
        let inputs: Vec<KeyedInput<usize>> = (0..80).map(|i| KeyedInput { key: i, value: 1000 }).collect();

        let start = std::time::Instant::now();
        let result = select(&inputs, 40_500u64, 1000).unwrap();

        assert!(start.elapsed().as_secs() < 5);
        // 41 x 1000 is the smallest total over the target, so a cut-short search that still returns it is one that
        // settles for the largest-first descent rather than for whatever it happened to hold at the cap.
        assert_eq!(result.total_value, 41_000);
        assert_eq!(result.selected_keys.len(), 41);
    }

    #[test]
    fn it_returns_a_largest_first_selection_when_the_search_is_cut_short() {
        let inputs = vec![
            KeyedInput { key: "A", value: 100 },
            KeyedInput { key: "B", value: 400 },
            KeyedInput { key: "C", value: 300 },
            KeyedInput { key: "D", value: 200 },
        ];

        let result = select_bounded(&inputs, 500u64, 1000, 0).unwrap();
        assert_eq!(result.total_value, 700);
        assert_eq!(result.selected_keys, vec![&"B", &"C"]);
    }

    #[test]
    fn a_cut_short_search_respects_max_inputs() {
        let inputs = vec![
            KeyedInput { key: "A", value: 100 },
            KeyedInput { key: "B", value: 400 },
            KeyedInput { key: "C", value: 300 },
            KeyedInput { key: "D", value: 200 },
        ];

        // Reachable within the limit, so the search runs and is cut short
        let result = select_bounded(&inputs, 650u64, 2, 0).unwrap();
        assert_eq!(result.selected_keys.len(), 2);
        assert_eq!(result.total_value, 700);
    }

    #[test]
    fn it_reports_an_unreachable_target_without_searching() {
        let inputs: Vec<KeyedInput<usize>> = (0..80).map(|i| KeyedInput { key: i, value: 1000 }).collect();

        // More than the inputs hold
        assert!(select(&inputs, 80_001u64, 1000).is_none());
        // Within the balance, but not within `max_inputs` of the inputs
        assert!(select(&inputs, 40_500u64, 40).is_none());
        assert!(select_bounded(&inputs, 40_500u64, 40, 0).is_none());
        // ...and exactly `max_inputs` of them do reach it
        assert!(select(&inputs, 40_000u64, 40).is_some());
    }
}
