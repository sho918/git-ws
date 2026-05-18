pub fn truncate_end(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let len = value.chars().count();
    if len <= width {
        return value.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }

    let mut out: String = value.chars().take(width - 1).collect();
    out.push('…');
    out
}

pub fn pad_truncate(value: &str, width: usize) -> String {
    format!("{:<width$}", truncate_end(value, width))
}

#[derive(Debug, Clone, Copy)]
pub struct ColumnWidth {
    desired: usize,
    minimum: usize,
    grow_weight: usize,
    shrink_order: usize,
}

impl ColumnWidth {
    pub const fn new(
        desired: usize,
        minimum: usize,
        grow_weight: usize,
        shrink_order: usize,
    ) -> Self {
        Self {
            desired,
            minimum,
            grow_weight,
            shrink_order,
        }
    }

    pub const fn fixed(width: usize) -> Self {
        Self {
            desired: width,
            minimum: width,
            grow_weight: 0,
            shrink_order: usize::MAX,
        }
    }
}

pub fn fit_priority_column_widths(columns: &[ColumnWidth], total_width: usize) -> Vec<usize> {
    if columns.is_empty() {
        return Vec::new();
    }

    let gaps = columns.len().saturating_sub(1);
    let max_content_width = total_width.saturating_sub(gaps);
    let mut widths: Vec<usize> = columns
        .iter()
        .map(|column| column.desired.max(column.minimum))
        .collect();

    shrink_priority_to(&mut widths, columns, max_content_width);
    grow_priority_to(&mut widths, columns, max_content_width);
    widths
}

fn shrink_priority_to(widths: &mut [usize], columns: &[ColumnWidth], max_content_width: usize) {
    shrink_while_too_wide(widths, max_content_width, |widths| {
        priority_shrinkable(widths, columns)
    });
    shrink_while_too_wide(widths, max_content_width, widest_non_empty);
}

fn shrink_while_too_wide(
    widths: &mut [usize],
    max_content_width: usize,
    mut choose: impl FnMut(&[usize]) -> Option<usize>,
) {
    while widths.iter().sum::<usize>() > max_content_width {
        let Some(index) = choose(widths) else {
            break;
        };
        widths[index] -= 1;
    }
}

fn grow_priority_to(widths: &mut [usize], columns: &[ColumnWidth], max_content_width: usize) {
    let grow_indices = weighted_grow_indices(columns);
    if grow_indices.is_empty() {
        return;
    }

    let mut cursor = 0;
    while widths.iter().sum::<usize>() < max_content_width {
        let index = grow_indices[cursor % grow_indices.len()];
        widths[index] += 1;
        cursor += 1;
    }
}

fn weighted_grow_indices(columns: &[ColumnWidth]) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .flat_map(|(index, column)| std::iter::repeat_n(index, column.grow_weight))
        .collect()
}

fn priority_shrinkable(widths: &[usize], columns: &[ColumnWidth]) -> Option<usize> {
    widths
        .iter()
        .zip(columns)
        .enumerate()
        .filter(|(_, (width, column))| **width > column.minimum)
        .min_by_key(|(_, (width, column))| (column.shrink_order, std::cmp::Reverse(**width)))
        .map(|(index, _)| index)
}

fn widest_non_empty(widths: &[usize]) -> Option<usize> {
    widths
        .iter()
        .enumerate()
        .filter(|(_, width)| **width > 0)
        .max_by_key(|(_, width)| *width)
        .map(|(index, _)| index)
}

pub fn terminal_columns(default: usize) -> usize {
    crossterm::terminal::size()
        .map(|(columns, _)| terminal_columns_from_size(columns, default))
        .unwrap_or(default)
}

fn terminal_columns_from_size(columns: u16, default: usize) -> usize {
    match columns {
        0 => default,
        columns => usize::from(columns),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_values_with_ellipsis() {
        assert_eq!(truncate_end("feature/very-long-branch", 12), "feature/ver…");
    }

    #[test]
    fn leaves_values_that_fit_unchanged() {
        assert_eq!(truncate_end("feature", 8), "feature");
    }

    #[test]
    fn handles_tiny_widths_without_panicking() {
        assert_eq!(truncate_end("feature", 0), "");
        assert_eq!(truncate_end("feature", 1), "…");
    }

    #[test]
    fn fits_priority_columns_inside_total_width_including_gaps() {
        let widths = fit_priority_column_widths(
            &[
                ColumnWidth::new(12, 6, 0, 0),
                ColumnWidth::new(34, 10, 1, 1),
                ColumnWidth::new(28, 8, 0, 2),
                ColumnWidth::new(16, 6, 0, 3),
            ],
            52,
        );

        assert_eq!(widths.len(), 4);
        assert!(
            widths.iter().sum::<usize>() + widths.len() - 1 <= 52,
            "{widths:?}"
        );
        assert!(
            widths
                .iter()
                .zip([6, 10, 8, 6])
                .all(|(width, min)| *width >= min)
        );
    }

    #[test]
    fn fits_priority_columns_by_growing_important_columns_first() {
        let widths = fit_priority_column_widths(
            &[
                ColumnWidth::fixed(7),
                ColumnWidth::new(18, 12, 1, 3),
                ColumnWidth::new(18, 10, 1, 2),
                ColumnWidth::new(10, 8, 1, 1),
                ColumnWidth::fixed(7),
                ColumnWidth::new(12, 8, 3, 4),
                ColumnWidth::fixed(6),
                ColumnWidth::new(12, 8, 4, 5),
            ],
            140,
        );

        assert_eq!(widths.iter().sum::<usize>() + widths.len() - 1, 140);
        assert!(widths[7] > widths[1], "{widths:?}");
        assert!(widths[5] > widths[2], "{widths:?}");
    }

    #[test]
    fn terminal_columns_uses_default_when_reported_width_is_zero() {
        assert_eq!(terminal_columns_from_size(0, 140), 140);
    }
}
