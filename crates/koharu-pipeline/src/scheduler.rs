use std::collections::BTreeMap;

use koharu_scene::EntityId;

use crate::Stage;

#[derive(Clone, Copy, Eq, PartialEq)]
enum WorkState {
    Pending,
    Running,
    Finished,
}

struct StageWork {
    stage: Stage,
    state: WorkState,
}

struct PageWork {
    page: EntityId,
    stages: Vec<StageWork>,
}

impl PageWork {
    fn finished(&self) -> bool {
        self.stages
            .iter()
            .all(|work| work.state == WorkState::Finished)
    }

    fn ready(&self, index: usize) -> bool {
        let Some(prerequisite) = prerequisite(self.stages[index].stage) else {
            return true;
        };
        self.stages
            .iter()
            .find(|work| work.stage == prerequisite)
            .is_none_or(|work| work.state == WorkState::Finished)
    }
}

/// Signals produced by [`Scheduler::complete_stage`] so the caller can react
/// to page- and stage-level milestones (dropping cached page data, unloading
/// a stage's model once every page has finished it).
#[derive(Default, Clone, Copy, Eq, PartialEq)]
pub(crate) struct StageProgress {
    pub(crate) page_finished: bool,
    pub(crate) stage_finished: bool,
}

/// Schedules pipeline work one stage at a time: every page finishes a given
/// stage before any page is allowed to start the next one. This keeps at
/// most one stage's model resident on the accelerator at a time, instead of
/// interleaving stages across pages (which requires every stage's model to
/// stay loaded simultaneously).
pub(crate) struct Scheduler {
    pages: Vec<PageWork>,
    page_index: BTreeMap<EntityId, usize>,
    total: usize,
}

impl Scheduler {
    pub(crate) fn new(pages: &[EntityId], stages: &[Stage]) -> Self {
        let pages = pages
            .iter()
            .map(|page| PageWork {
                page: *page,
                stages: stages
                    .iter()
                    .map(|stage| StageWork {
                        stage: *stage,
                        state: WorkState::Pending,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let total = pages.len().saturating_mul(stages.len());
        Self {
            page_index: pages
                .iter()
                .enumerate()
                .map(|(index, page)| (page.page, index))
                .collect(),
            pages,
            total,
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.total
    }

    /// Returns the next `(page, stage)` job to run, if any. Stages are
    /// scanned in pipeline order (stage index 0 first); a stage is only
    /// considered once every earlier stage has *finished* (not merely
    /// started) for every page, so two different stages are never both
    /// in flight at once.
    pub(crate) fn start_next(&mut self) -> Option<(EntityId, Stage)> {
        let stage_count = self.pages.first().map_or(0, |page| page.stages.len());
        for stage_index in 0..stage_count {
            if stage_index > 0
                && !self.pages.iter().all(|page| {
                    page.stages[..stage_index]
                        .iter()
                        .all(|work| work.state == WorkState::Finished)
                })
            {
                // An earlier stage is still finishing up (e.g. its last job
                // is Running). Don't let a later stage jump ahead of it.
                return None;
            }
            for page_index in 0..self.pages.len() {
                let stage = self.pages[page_index].stages[stage_index].stage;
                if self.pages[page_index].stages[stage_index].state == WorkState::Pending
                    && self.pages[page_index].ready(stage_index)
                {
                    self.pages[page_index].stages[stage_index].state = WorkState::Running;
                    return Some((self.pages[page_index].page, stage));
                }
            }
        }
        None
    }

    /// Marks `stage` finished for `page`. Returns which milestones this
    /// completion crossed: whether `page` has no work left at all, and
    /// whether `stage` has no work left for *any* page (the signal callers
    /// use to unload that stage's model before the next stage begins).
    pub(crate) fn complete_stage(&mut self, page: EntityId, stage: Stage) -> StageProgress {
        let Some(&page_index) = self.page_index.get(&page) else {
            return StageProgress::default();
        };
        let target = &mut self.pages[page_index];
        let was_finished = target.finished();
        if let Some(work) = target.stages.iter_mut().find(|work| work.stage == stage) {
            work.state = WorkState::Finished;
        }
        let page_finished = !was_finished && target.finished();
        let stage_finished = self.pages.iter().all(|page| {
            page.stages
                .iter()
                .find(|work| work.stage == stage)
                .is_none_or(|work| work.state == WorkState::Finished)
        });
        StageProgress {
            page_finished,
            stage_finished,
        }
    }
}

const fn prerequisite(stage: Stage) -> Option<Stage> {
    match stage {
        Stage::Detection => None,
        Stage::Ocr | Stage::Inpainting => Some(Stage::Detection),
        Stage::Translation => Some(Stage::Ocr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(count: usize) -> Vec<EntityId> {
        (0..count).map(|_| EntityId::new()).collect()
    }

    #[test]
    fn finishes_one_stage_for_every_page_before_the_next_stage_starts() {
        let pages = pages(3);
        let stages = [Stage::Detection, Stage::Ocr, Stage::Inpainting];
        let mut scheduler = Scheduler::new(&pages, &stages);
        // Detection must run for every page before Ocr is even offered,
        // even though Detection has no cross-page dependency that would
        // otherwise force this ordering.
        for page in &pages {
            let job = scheduler.start_next().unwrap();
            assert_eq!(job, (*page, Stage::Detection));
            let progress = scheduler.complete_stage(*page, Stage::Detection);
            assert!(!progress.page_finished);
            assert_eq!(progress.stage_finished, *page == pages[2]);
        }

        // Now Ocr is available for every page, in order, and Detection is
        // never offered again.
        for page in &pages {
            let job = scheduler.start_next().unwrap();
            assert_eq!(job, (*page, Stage::Ocr));
            scheduler.complete_stage(*page, Stage::Ocr);
        }

        for page in &pages {
            let job = scheduler.start_next().unwrap();
            assert_eq!(job, (*page, Stage::Inpainting));
            let progress = scheduler.complete_stage(*page, Stage::Inpainting);
            assert!(progress.page_finished);
        }

        assert!(scheduler.start_next().is_none());
        assert_eq!(scheduler.total(), pages.len() * stages.len());
    }

    #[test]
    fn later_stage_waits_for_the_in_flight_job_of_an_earlier_stage() {
        let pages = pages(2);
        let stages = [Stage::Detection, Stage::Ocr];
        let mut scheduler = Scheduler::new(&pages, &stages);
        let first = scheduler.start_next().unwrap();
        assert_eq!(first, (pages[0], Stage::Detection));
        // pages[0]'s Detection job is still "Running" (no complete_stage
        // call yet), so Detection for pages[1] is the only valid next job -
        // Ocr must not be offered for pages[0] even though its own
        // prerequisite check would technically allow it once finished.
        let second = scheduler.start_next().unwrap();
        assert_eq!(second, (pages[1], Stage::Detection));
        assert!(scheduler.start_next().is_none());

        let progress = scheduler.complete_stage(pages[0], Stage::Detection);
        assert!(!progress.stage_finished);
        // pages[1]'s Detection is still Running, so the Ocr layer is still
        // blocked.
        assert!(scheduler.start_next().is_none());

        let progress = scheduler.complete_stage(pages[1], Stage::Detection);
        assert!(progress.stage_finished);
        let ocr = scheduler.start_next().unwrap();
        assert_eq!(ocr, (pages[0], Stage::Ocr));
    }

    #[test]
    fn single_stage_operation_has_no_layer_barrier() {
        let pages = pages(2);
        let stages = [Stage::Translation];
        let mut scheduler = Scheduler::new(&pages, &stages);
        let busy = BTreeSet::new();

        let first = scheduler.start_next().unwrap();
        assert_eq!(first, (pages[0], Stage::Translation));
        let progress = scheduler.complete_stage(pages[0], Stage::Translation);
        assert!(progress.page_finished);
        assert!(!progress.stage_finished);

        let second = scheduler.start_next().unwrap();
        assert_eq!(second, (pages[1], Stage::Translation));
        let progress = scheduler.complete_stage(pages[1], Stage::Translation);
        assert!(progress.stage_finished);
    }
}