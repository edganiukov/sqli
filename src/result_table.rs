/// Result table layout helpers used by both rendering and navigation.
use std::cmp::Reverse;

pub const CELL_PADDING: usize = 2;
pub const MIN_SHRUNK_COL_WIDTH: usize = 6;

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
pub fn fit_column_widths(widths: &[usize], available_width: usize) -> Vec<usize> {
    if widths.is_empty() {
        return Vec::new();
    }

    let mut fitted = widths.to_vec();
    let mut total: usize = fitted.iter().sum();

    if total <= available_width {
        return fitted;
    }

    // A short natural column should stay short; long ones can shrink down to this floor.
    let lower_bounds: Vec<usize> = widths
        .iter()
        .map(|&w| w.min(MIN_SHRUNK_COL_WIDTH.max(1)))
        .collect();
    let min_total: usize = lower_bounds.iter().sum();

    if min_total >= available_width {
        return lower_bounds;
    }

    let mut low = lower_bounds.iter().copied().max().unwrap_or(1);
    let mut high = widths.iter().copied().max().unwrap_or(low);

    while low < high {
        let mid = (low + high) / 2;
        let sum = capped_sum(widths, &lower_bounds, mid);
        if sum <= available_width {
            high = mid;
        } else {
            low = mid + 1;
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
    fit_column_widths(&measured, available_width)
}

fn capped_sum(widths: &[usize], lower_bounds: &[usize], cap: usize) -> usize {
    widths
        .iter()
        .zip(lower_bounds.iter())
        .map(|(&w, &lb)| w.min(cap).max(lb))
        .sum()
}
