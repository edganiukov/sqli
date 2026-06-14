/// Result table layout helpers used by both rendering and navigation.
use std::cmp::Reverse;
use std::collections::HashMap;

pub const CELL_PADDING: usize = 2;
/// Absolute floor for any column, so even tiny columns stay readable.
pub const MIN_COL_WIDTH: usize = 8;
/// A long header won't push a column's floor past this (it truncates instead),
/// so one verbose name can't crush every other column.
pub const MAX_NAME_FLOOR: usize = 24;

/// Lower bound for a single column: wide enough to show its header (the renderer
/// keeps `width - 1` characters), clamped to a readable range, but never more than
/// the column naturally needs. Keeping this header-aware means that when there are
/// too many columns to fit, names stay visible and the surplus scrolls horizontally
/// instead of every column collapsing to an unreadable stub.
fn column_floor(natural_width: usize, header_len: usize) -> usize {
    let name_floor = (header_len + 1).clamp(MIN_COL_WIDTH, MAX_NAME_FLOOR);
    natural_width.min(name_floor)
}

/// Measure natural column widths from headers + row values.
///
/// Widths include a small right-side padding so cells breathe.
pub fn measure_column_widths(columns: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = columns.iter().map(|h| h.len() + CELL_PADDING).collect();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len() + CELL_PADDING);
            }
        }
    }

    widths
}

/// Fit column widths into an available width.
///
/// Strategy:
/// 1) Keep full widths when everything fits.
/// 2) If not, shrink the longest columns first (down to per-column lower bounds).
/// 3) If still not enough, keep lower bounds and rely on horizontal scrolling.
///
/// `lower_bounds[i]` is the smallest width column `i` may shrink to; the caller
/// derives these so column headers stay readable (see [`column_floor`]).
pub fn fit_column_widths(
    widths: &[usize],
    lower_bounds: &[usize],
    available_width: usize,
) -> Vec<usize> {
    if widths.is_empty() {
        return Vec::new();
    }

    let mut fitted = widths.to_vec();
    let mut total: usize = fitted.iter().sum();

    if total <= available_width {
        return fitted;
    }

    let min_total: usize = lower_bounds.iter().sum();

    if min_total >= available_width {
        // Too many columns to fit even at their floors: keep them readable and let
        // the overflow scroll horizontally rather than collapsing every column.
        return lower_bounds.to_vec();
    }

    // Largest uniform per-column cap whose capped total still fits. `capped_sum`
    // is non-decreasing in the cap, and `capped_sum(0)` equals `min_total` (< available
    // here), so cap = 0 is always feasible and the search is well-defined.
    let mut low = 0;
    let mut high = widths.iter().copied().max().unwrap_or(0);

    while low < high {
        let mid = (low + high).div_ceil(2);
        if capped_sum(widths, lower_bounds, mid) <= available_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    let cap = low;
    for (i, slot) in fitted.iter_mut().enumerate() {
        *slot = widths[i].min(cap).max(lower_bounds[i]);
    }

    total = fitted.iter().sum();
    let mut remaining = available_width.saturating_sub(total);

    if remaining > 0 {
        let mut growable: Vec<usize> = (0..fitted.len())
            .filter(|&i| fitted[i] < widths[i])
            .collect();
        growable.sort_by_key(|&i| Reverse(widths[i]));

        while remaining > 0 {
            let mut grew = false;
            for &idx in &growable {
                if remaining == 0 {
                    break;
                }
                if fitted[idx] < widths[idx] {
                    fitted[idx] += 1;
                    remaining -= 1;
                    grew = true;
                }
            }

            if !grew {
                break;
            }
        }
    }

    fitted
}

/// Build final base widths for the query result table.
pub fn result_table_widths(
    columns: &[String],
    rows: &[Vec<String>],
    available_width: usize,
) -> Vec<usize> {
    let measured = measure_column_widths(columns, rows);
    let lower_bounds: Vec<usize> = columns
        .iter()
        .zip(measured.iter())
        .map(|(col, &natural)| column_floor(natural, col.chars().count()))
        .collect();
    fit_column_widths(&measured, &lower_bounds, available_width)
}

/// Like [`result_table_widths`], but applies user resize overrides on top of the
/// auto-fit widths. Both the renderer and keyboard column navigation use this so
/// their column geometry (and horizontal scroll math) stay in sync.
pub fn result_table_widths_with_overrides(
    columns: &[String],
    rows: &[Vec<String>],
    available_width: usize,
    overrides: &HashMap<usize, usize>,
) -> Vec<usize> {
    let mut widths = result_table_widths(columns, rows, available_width);
    if !overrides.is_empty() {
        for (&i, &w) in overrides {
            if let Some(slot) = widths.get_mut(i) {
                *slot = w;
            }
        }
    }
    widths
}

fn capped_sum(widths: &[usize], lower_bounds: &[usize], cap: usize) -> usize {
    widths
        .iter()
        .zip(lower_bounds.iter())
        .map(|(&w, &lb)| w.min(cap).max(lb))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn keeps_full_widths_when_everything_fits() {
        let columns = cols(&["id", "name"]);
        let rows = vec![vec!["1".to_string(), "alice".to_string()]];
        let widths = result_table_widths(&columns, &rows, 200);
        assert_eq!(widths, measure_column_widths(&columns, &rows));
    }

    #[test]
    fn many_columns_overflow_instead_of_collapsing() {
        // 20 columns with readable headers can't fit an 80-col viewport.
        let names: Vec<String> = (0..20).map(|i| format!("column_{i:02}")).collect();
        let columns: Vec<String> = names.clone();
        let rows = vec![names.iter().map(|_| "value".to_string()).collect()];

        let available = 80;
        let widths = result_table_widths(&columns, &rows, available);

        // Each column stays wide enough to show its full header, so the names are
        // never truncated...
        for (col, &w) in columns.iter().zip(&widths) {
            assert!(
                w >= col.chars().count() + 1,
                "column '{col}' width {w} hides its header",
            );
        }
        // ...and the total overflows the viewport so horizontal scroll kicks in.
        assert!(widths.iter().sum::<usize>() > available);
    }

    #[test]
    fn header_floor_is_capped_for_very_long_names() {
        let long = "a".repeat(60);
        let columns = cols(&[long.as_str(), "x"]);
        let rows = vec![vec!["v".to_string(), "v".to_string()]];
        let widths = result_table_widths(&columns, &rows, 20);
        assert!(widths[0] <= MAX_NAME_FLOOR);
    }

    #[test]
    fn overrides_replace_auto_widths() {
        let columns = cols(&["id", "name"]);
        let rows = vec![vec!["1".to_string(), "alice".to_string()]];
        let mut overrides = HashMap::new();
        overrides.insert(1, 40);
        let widths = result_table_widths_with_overrides(&columns, &rows, 200, &overrides);
        assert_eq!(widths[1], 40);
        // Out-of-range overrides are ignored, not panics.
        overrides.insert(99, 10);
        let widths = result_table_widths_with_overrides(&columns, &rows, 200, &overrides);
        assert_eq!(widths.len(), 2);
    }

    #[test]
    fn shrinks_long_columns_to_fit_without_scroll() {
        // A couple of value-heavy columns with short headers should shrink to fit
        // rather than forcing horizontal scroll.
        let columns = cols(&["a", "b"]);
        let rows = vec![vec!["x".repeat(60), "y".repeat(60)]];
        let available = 60;
        let widths = result_table_widths(&columns, &rows, available);
        assert!(widths.iter().sum::<usize>() <= available);
        // Short headers ("a", "b") remain visible.
        assert!(widths.iter().all(|&w| w >= MIN_COL_WIDTH));
    }
}
