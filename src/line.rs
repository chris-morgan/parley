//! Greedy line breaking.

use super::data::*;
use super::layout::*;

use core::ops::Range;

#[derive(Default)]
struct LineLayout {
    lines: Vec<LineData>,
    runs: Vec<LineRunData>,
}

impl LineLayout {
    fn swap<B>(&mut self, layout: &mut LayoutData<B>) {
        core::mem::swap(&mut self.lines, &mut layout.lines);
        core::mem::swap(&mut self.runs, &mut layout.line_runs);
    }
}

/// Line breaking support for a paragraph.
pub struct BreakLines<'a, B> {
    layout: &'a mut LayoutData<B>,
    lines: LineLayout,
    state: BreakerState,
    prev_state: Option<BreakerState>,
}

impl<'a, B> BreakLines<'a, B> {
    pub(crate) fn new(layout: &'a mut LayoutData<B>) -> Self {
        let mut lines = LineLayout::default();
        lines.swap(layout);
        lines.lines.clear();
        lines.runs.clear();
        Self {
            layout,
            lines,
            state: BreakerState::default(),
            prev_state: None,
        }
    }

    /// Computes the next line in the paragraph. Returns the advance and size
    /// (width and height for horizontal layouts) of the line.
    pub fn break_next(&mut self, max_advance: f32, alignment: Alignment) -> Option<(f32, f32)> {
        use swash::text::cluster::Boundary;
        self.prev_state = Some(self.state.clone());
        let run_count = self.layout.runs.len();
        while self.state.i < run_count {
            let run_data = &self.layout.runs[self.state.i];
            let run = Run::new(self.layout, run_data, None);
            let cluster_start = run_data.cluster_range.start;
            let cluster_end = run_data.cluster_range.end;
            while self.state.j < cluster_end {
                let cluster = run.get(self.state.j - cluster_start).unwrap();
                let boundary = cluster.info().boundary();
                match boundary {
                    Boundary::Mandatory => {
                        if !self.state.line.skip_mandatory_break {
                            self.state.prev_boundary = None;
                            self.state.line.clusters.end = self.state.j;
                            self.state.line.runs.end = self.state.i + 1;
                            self.state.line.skip_mandatory_break = true;
                            if commit_line(
                                self.layout,
                                &mut self.lines,
                                &mut self.state.line,
                                max_advance,
                                alignment,
                                true,
                            ) {
                                self.state.runs = self.lines.runs.len();
                                self.state.lines = self.lines.lines.len();
                                self.state.line.x = 0.;
                                let line = self.lines.lines.last().unwrap();
                                return Some((line.metrics.advance, line.size()));
                            }
                        }
                    }
                    Boundary::Line => {
                        self.state.prev_boundary = Some(PrevBoundaryState {
                            i: self.state.i,
                            j: self.state.j,
                            state: self.state.line.clone(),
                        });
                    }
                    _ => {}
                }
                self.state.line.skip_mandatory_break = false;
                let mut advance = cluster.advance();
                if cluster.is_ligature_start() {
                    while (self.state.j + 1) < cluster_end {
                        let cluster = run.get(self.state.j + 1).unwrap();
                        if !cluster.is_ligature_continuation() {
                            break;
                        } else {
                            advance += cluster.advance();
                            self.state.j += 1;
                        }
                    }
                }
                let next_x = self.state.line.x + advance;
                if next_x > max_advance {
                    if cluster.info().whitespace().is_space_or_nbsp() {
                        // Hang overflowing whitespace
                        self.state.line.runs.end = self.state.i + 1;
                        self.state.line.clusters.end = self.state.j + 1;
                        self.state.line.x = next_x;
                        if commit_line(
                            self.layout,
                            &mut self.lines,
                            &mut self.state.line,
                            max_advance,
                            alignment,
                            false,
                        ) {
                            self.state.runs = self.lines.runs.len();
                            self.state.lines = self.lines.lines.len();
                            self.state.line.x = 0.;
                            let line = self.lines.lines.last().unwrap();
                            self.state.prev_boundary = None;
                            self.state.j += 1;
                            return Some((line.metrics.advance, line.size()));
                        }
                    } else if let Some(prev) = self.state.prev_boundary.take() {
                        if prev.state.x == 0. {
                            // This will cycle if we try to rewrap. Accept the overflowing fragment.
                            self.state.line.runs.end = self.state.i + 1;
                            self.state.line.clusters.end = self.state.j + 1;
                            self.state.line.x = next_x;
                            self.state.j += 1;
                            if commit_line(
                                self.layout,
                                &mut self.lines,
                                &mut self.state.line,
                                max_advance,
                                alignment,
                                false,
                            ) {
                                self.state.runs = self.lines.runs.len();
                                self.state.lines = self.lines.lines.len();
                                self.state.line.x = 0.;
                                let line = self.lines.lines.last().unwrap();
                                self.state.prev_boundary = None;
                                self.state.j += 1;
                                return Some((line.metrics.advance, line.size()));
                            }
                        } else {
                            self.state.line = prev.state;
                            if commit_line(
                                self.layout,
                                &mut self.lines,
                                &mut self.state.line,
                                max_advance,
                                alignment,
                                false,
                            ) {
                                self.state.runs = self.lines.runs.len();
                                self.state.lines = self.lines.lines.len();
                                self.state.line.x = 0.;
                                let line = self.lines.lines.last().unwrap();
                                self.state.i = prev.i;
                                self.state.j = prev.j;
                                return Some((line.metrics.advance, line.size()));
                            }
                        }
                    } else {
                        if self.state.line.x == 0. {
                            // If we're at the start of the line, this particular
                            // cluster will never fit, so consume it and accept
                            // the overflow.
                            self.state.line.runs.end = self.state.i + 1;
                            self.state.line.clusters.end = self.state.j + 1;
                            self.state.line.x = next_x;
                            self.state.j += 1;
                        }
                        if commit_line(
                            self.layout,
                            &mut self.lines,
                            &mut self.state.line,
                            max_advance,
                            alignment,
                            false,
                        ) {
                            self.state.runs = self.lines.runs.len();
                            self.state.lines = self.lines.lines.len();
                            self.state.line.x = 0.;
                            let line = self.lines.lines.last().unwrap();
                            self.state.prev_boundary = None;
                            self.state.j += 1;
                            return Some((line.metrics.advance, line.size()));
                        }
                    }
                } else {
                    // Commit the cluster to the line.
                    self.state.line.runs.end = self.state.i + 1;
                    self.state.line.clusters.end = self.state.j + 1;
                    self.state.line.x = next_x;
                    self.state.j += 1;
                }
            }
            self.state.i += 1;
        }
        if commit_line(
            self.layout,
            &mut self.lines,
            &mut self.state.line,
            max_advance,
            alignment,
            true,
        ) {
            self.state.runs = self.lines.runs.len();
            self.state.lines = self.lines.lines.len();
            self.state.line.x = 0.;
            let line = self.lines.lines.last().unwrap();
            return Some((line.metrics.advance, line.size()));
        }
        None
    }

    /// Reverts the last computed line, returning to the previous state.
    pub fn revert(&mut self) -> bool {
        if let Some(state) = self.prev_state.take() {
            self.state = state;
            self.lines.lines.truncate(self.state.lines);
            self.lines.runs.truncate(self.state.runs);
            true
        } else {
            false
        }
    }

    /// Breaks all remaining lines with the specified maximum advance. This
    /// consumes the line breaker.
    pub fn break_remaining(mut self, max_advance: f32, alignment: Alignment) {
        while self.break_next(max_advance, alignment).is_some() {}
        self.finish();
    }

    /// Consumes the line breaker and finalizes all line computations.
    pub fn finish(mut self) {
        for run in &mut self.lines.runs {
            run.is_whitespace = true;
            if run.bidi_level & 1 != 0 {
                // RTL runs check for "trailing" whitespace at the front.
                for cluster in self.layout.clusters[run.cluster_range.clone()].iter() {
                    if cluster.info.is_whitespace() {
                        run.has_trailing_whitespace = true;
                    } else {
                        run.is_whitespace = false;
                        break;
                    }
                }
            } else {
                for cluster in self.layout.clusters[run.cluster_range.clone()].iter().rev() {
                    if cluster.info.is_whitespace() {
                        run.has_trailing_whitespace = true;
                    } else {
                        run.is_whitespace = false;
                        break;
                    }
                }
            }
        }
        let mut y = 0.;
        for line in &mut self.lines.lines {
            let run_base = line.run_range.start;
            let run_count = line.run_range.end - run_base;
            line.metrics.ascent = 0.;
            line.metrics.descent = 0.;
            line.metrics.leading = 0.;
            line.metrics.offset = 0.;
            let mut have_metrics = false;
            let mut needs_reorder = false;
            line.text_range.start = usize::MAX;
            // Compute metrics for the line, but ignore trailing whitespace.
            for line_run in self.lines.runs[line.run_range.clone()].iter().rev() {
                line.text_range.end = line.text_range.end.max(line_run.text_range.end);
                line.text_range.start = line.text_range.start.min(line_run.text_range.start);
                if line_run.bidi_level != 0 {
                    needs_reorder = true;
                }
                if !have_metrics && line_run.is_whitespace {
                    continue;
                }
                let run = &self.layout.runs[line_run.run_index];
                line.metrics.ascent = line.metrics.ascent.max(run.metrics.ascent);
                line.metrics.descent = line.metrics.descent.max(run.metrics.descent);
                line.metrics.leading = line.metrics.leading.max(run.metrics.leading);
                have_metrics = true;
            }
            if needs_reorder && run_count > 1 {
                reorder_runs(&mut self.lines.runs[line.run_range.clone()]);
            }
            let trailing_whitespace = if !line.run_range.is_empty() {
                let last_run = &self.lines.runs[line.run_range.end - 1];
                if !last_run.cluster_range.is_empty() {
                    let cluster = &self.layout.clusters[last_run.cluster_range.end - 1];
                    if cluster.info.whitespace().is_space_or_nbsp() {
                        cluster.advance
                    } else {
                        0.
                    }
                } else {
                    0.
                }
            } else {
                0.
            };
            line.metrics.trailing_whitespace = trailing_whitespace;
            if line.alignment != Alignment::Start {
                let extra = line.max_advance - line.metrics.advance + trailing_whitespace;
                if extra > 0. {
                    let offset = if line.alignment == Alignment::Middle {
                        extra * 0.5
                    } else {
                        extra
                    };
                    line.metrics.offset = offset;
                }
            }
            // if line.explicit_break {
            //     // self.lines.clusters.get_mut(line.clusters.1.saturating_sub(1) as usize).map(|c| c.flags |= CLUSTER_NEWLINE);
            // }
            if !have_metrics {
                // Line consisting entirely of whitespace?
                if !line.run_range.is_empty() {
                    let line_run = &self.lines.runs[line.run_range.start];
                    let run = &self.layout.runs[line_run.run_index];
                    line.metrics.ascent = run.metrics.ascent;
                    line.metrics.descent = run.metrics.descent;
                    line.metrics.leading = run.metrics.leading;
                }
            }
            line.metrics.ascent = line.metrics.ascent.round();
            line.metrics.descent = line.metrics.descent.round();
            line.metrics.leading = (line.metrics.leading * 0.5).round() * 2.;
            let above = (line.metrics.ascent + line.metrics.leading * 0.5).round();
            let below = (line.metrics.descent + line.metrics.leading * 0.5).round();
            line.metrics.baseline = y + above;
            y = line.metrics.baseline + below;
        }
    }
}

impl<'a, B> Drop for BreakLines<'a, B> {
    fn drop(&mut self) {
        self.lines.swap(self.layout);
    }
}

#[derive(Clone, Default)]
struct LineState {
    x: f32,
    runs: Range<usize>,
    clusters: Range<usize>,
    skip_mandatory_break: bool,
}

#[derive(Clone, Default)]
struct PrevBoundaryState {
    i: usize,
    j: usize,
    state: LineState,
}

#[derive(Clone, Default)]
struct BreakerState {
    runs: usize,
    lines: usize,
    i: usize,
    j: usize,
    line: LineState,
    prev_boundary: Option<PrevBoundaryState>,
}

fn commit_line<B>(
    layout: &LayoutData<B>,
    lines: &mut LineLayout,
    state: &mut LineState,
    max_advance: f32,
    alignment: Alignment,
    explicit: bool,
) -> bool {
    state.clusters.end = state.clusters.end.min(layout.clusters.len());
    if state.runs.is_empty() || state.clusters.is_empty() {
        return false;
    }
    // let line_index = lines.lines.len();
    let last_run = state.runs.len() - 1;
    let runs_start = lines.runs.len();
    for (i, run_data) in layout.runs[state.runs.clone()].iter().enumerate() {
        let run_index = state.runs.start + i;
        let mut cluster_range = run_data.cluster_range.clone();
        if i == 0 {
            cluster_range.start = state.clusters.start;
        }
        if i == last_run {
            cluster_range.end = state.clusters.end;
        }
        if cluster_range.start >= cluster_range.end {
            continue;
        }
        let run = Run::new(layout, run_data, None);
        let first_cluster = run
            .get(cluster_range.start - run_data.cluster_range.start)
            .unwrap();
        let last_cluster = run
            .get(cluster_range.end - run_data.cluster_range.start - 1)
            .unwrap();
        let text_range = first_cluster.text_range().start..last_cluster.text_range().end;
        let line_run = LineRunData {
            run_index,
            bidi_level: run_data.bidi_level,
            is_whitespace: false,
            has_trailing_whitespace: false,
            cluster_range,
            text_range,
        };
        lines.runs.push(line_run);
    }
    let runs_end = lines.runs.len();
    if runs_start == runs_end {
        return false;
    }
    let mut line = LineData::default();
    line.run_range = runs_start..runs_end;
    line.metrics.advance = state.x;
    line.max_advance = max_advance;
    line.alignment = alignment;
    line.explicit_break = explicit;
    lines.lines.push(line);
    state.clusters.start = state.clusters.end;
    state.clusters.end += 1;
    state.runs.start = state.runs.end - 1;
    true
}

fn reorder_runs(runs: &mut [LineRunData]) {
    let mut max_level = 0;
    let mut lowest_odd_level = 255;
    let len = runs.len();
    for run in runs.iter() {
        let level = run.bidi_level;
        if level > max_level {
            max_level = level;
        }
        if level & 1 != 0 && level < lowest_odd_level {
            lowest_odd_level = level;
        }
    }
    for level in (lowest_odd_level..=max_level).rev() {
        let mut i = 0;
        while i < len {
            if runs[i].bidi_level >= level {
                let mut end = i + 1;
                while end < len && runs[end].bidi_level >= level {
                    end += 1;
                }
                let mut j = i;
                let mut k = end - 1;
                while j < k {
                    runs.swap(j, k);
                    j += 1;
                    k -= 1;
                }
                i = end;
            }
            i += 1;
        }
    }
}
