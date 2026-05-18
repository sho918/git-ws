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

pub fn fit_column_widths(desired: &[usize], minimum: &[usize], total_width: usize) -> Vec<usize> {
    debug_assert_eq!(desired.len(), minimum.len());
    if desired.is_empty() {
        return Vec::new();
    }

    let gaps = desired.len().saturating_sub(1);
    let max_content_width = total_width.saturating_sub(gaps);
    let mut widths: Vec<usize> = desired
        .iter()
        .zip(minimum)
        .map(|(desired, minimum)| (*desired).max(*minimum))
        .collect();

    shrink_to(&mut widths, minimum, max_content_width);
    widths
}

fn shrink_to(widths: &mut [usize], minimum: &[usize], max_content_width: usize) {
    while widths.iter().sum::<usize>() > max_content_width {
        let Some(index) = widest_shrinkable(widths, minimum) else {
            break;
        };
        widths[index] -= 1;
    }

    while widths.iter().sum::<usize>() > max_content_width {
        let Some(index) = widest_non_empty(widths) else {
            break;
        };
        widths[index] -= 1;
    }
}

fn widest_shrinkable(widths: &[usize], minimum: &[usize]) -> Option<usize> {
    widths
        .iter()
        .zip(minimum)
        .enumerate()
        .filter(|(_, (width, minimum))| width > minimum)
        .max_by_key(|(_, (width, _))| *width)
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
        .map(|(columns, _)| usize::from(columns))
        .unwrap_or(default)
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
    fn fits_columns_inside_total_width_including_gaps() {
        let widths = fit_column_widths(&[12, 34, 28, 16], &[6, 10, 8, 6], 52);

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
}
