use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum TestState {
    Initial,
    ClusterCreating,
    ImagesLoading,
    CRDsInstalling,
    InfrastructureReady,
    MinioDeploying,
    OperatorDeploying,
    ComponentsReady,
    TestExecuting,
    Cleanup,
    Complete,
    Failed(String),
}

impl TestState {
    pub fn description(&self) -> &'static str {
        match self {
            TestState::Initial => "Initializing test environment",
            TestState::ClusterCreating => "Creating Kind cluster",
            TestState::ImagesLoading => "Loading container images",
            TestState::CRDsInstalling => "Installing Custom Resource Definitions",
            TestState::InfrastructureReady => "Infrastructure components ready",
            TestState::MinioDeploying => "Deploying MinIO storage",
            TestState::OperatorDeploying => "Deploying Neon operator",
            TestState::ComponentsReady => "All components ready for testing",
            TestState::TestExecuting => "Executing test cases",
            TestState::Cleanup => "Cleaning up test environment",
            TestState::Complete => "Test completed successfully",
            TestState::Failed(_) => "Test failed",
        }
    }

    pub fn progress_percentage(&self) -> u8 {
        match self {
            TestState::Initial => 0,
            TestState::ClusterCreating => 10,
            TestState::ImagesLoading => 20,
            TestState::CRDsInstalling => 30,
            TestState::InfrastructureReady => 40,
            TestState::MinioDeploying => 50,
            TestState::OperatorDeploying => 70,
            TestState::ComponentsReady => 80,
            TestState::TestExecuting => 90,
            TestState::Cleanup => 95,
            TestState::Complete => 100,
            TestState::Failed(_) => 0,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, TestState::Complete | TestState::Failed(_))
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, TestState::Failed(_))
    }
}

#[derive(Debug)]
pub struct StateTransition {
    pub from: TestState,
    pub to: TestState,
    pub timestamp: Instant,
    pub duration: Option<Duration>,
}

pub struct TestStateMachine {
    current_state: TestState,
    start_time: Instant,
    transitions: Vec<StateTransition>,
    test_name: String,
}

impl TestStateMachine {
    pub fn new(test_name: &str) -> Self {
        info!(
            test_name = test_name,
            state = "Initial",
            progress = 0,
            "🚀 Starting test state machine"
        );

        Self {
            current_state: TestState::Initial,
            start_time: Instant::now(),
            transitions: Vec::new(),
            test_name: test_name.to_string(),
        }
    }

    pub fn current_state(&self) -> &TestState {
        &self.current_state
    }

    pub fn transition_to(&mut self, new_state: TestState) -> Result<(), String> {
        if self.current_state.is_terminal() {
            return Err(format!(
                "Cannot transition from terminal state {:?} to {:?}",
                self.current_state, new_state
            ));
        }

        let now = Instant::now();
        let duration = if let Some(last_transition) = self.transitions.last() {
            Some(now.duration_since(last_transition.timestamp))
        } else {
            Some(now.duration_since(self.start_time))
        };

        let transition = StateTransition {
            from: self.current_state.clone(),
            to: new_state.clone(),
            timestamp: now,
            duration,
        };

        // Log the transition with structured data
        let progress = new_state.progress_percentage();
        let total_duration = now.duration_since(self.start_time);

        info!(
            test_name = self.test_name,
            from_state = ?transition.from,
            to_state = ?transition.to,
            progress = progress,
            step_duration_ms = duration.map(|d| d.as_millis()).unwrap_or(0),
            total_duration_ms = total_duration.as_millis(),
            description = new_state.description(),
            "🔄 State transition"
        );

        if let Some(step_duration) = duration {
            debug!(
                test_name = self.test_name,
                from_state = ?transition.from,
                step_duration_ms = step_duration.as_millis(),
                "Step timing"
            );
        }

        self.transitions.push(transition);
        self.current_state = new_state;

        Ok(())
    }

    pub fn fail_with_error(&mut self, error: String) {
        let total_duration = Instant::now().duration_since(self.start_time);

        warn!(
            test_name = self.test_name,
            error = error,
            total_duration_ms = total_duration.as_millis(),
            final_state = ?self.current_state,
            "❌ Test failed"
        );

        self.current_state = TestState::Failed(error);
    }

    pub fn complete(&mut self) {
        let total_duration = Instant::now().duration_since(self.start_time);

        info!(
            test_name = self.test_name,
            total_duration_ms = total_duration.as_millis(),
            total_transitions = self.transitions.len(),
            "✅ Test completed successfully"
        );

        if let Err(e) = self.transition_to(TestState::Complete) {
            warn!("Failed to transition to Complete state: {}", e);
            self.current_state = TestState::Complete;
        }
    }

    pub fn get_timing_summary(&self) -> TimingSummary {
        let total_duration = Instant::now().duration_since(self.start_time);
        let mut step_timings = Vec::new();

        for transition in &self.transitions {
            if let Some(duration) = transition.duration {
                step_timings.push(StepTiming {
                    step: transition.to.description().to_string(),
                    duration,
                });
            }
        }

        TimingSummary {
            test_name: self.test_name.clone(),
            total_duration,
            step_timings,
            final_state: self.current_state.clone(),
        }
    }

    pub fn log_progress(&self, message: &str) {
        let progress = self.current_state.progress_percentage();
        let elapsed = Instant::now().duration_since(self.start_time);

        info!(
            test_name = self.test_name,
            state = ?self.current_state,
            progress = progress,
            elapsed_ms = elapsed.as_millis(),
            message = message,
            "📊 Progress update"
        );
    }
}

#[derive(Debug)]
pub struct TimingSummary {
    pub test_name: String,
    pub total_duration: Duration,
    pub step_timings: Vec<StepTiming>,
    pub final_state: TestState,
}

#[derive(Debug)]
pub struct StepTiming {
    pub step: String,
    pub duration: Duration,
}

impl TimingSummary {
    pub fn log_summary(&self) {
        info!(
            test_name = self.test_name,
            total_duration_ms = self.total_duration.as_millis(),
            final_state = ?self.final_state,
            "📈 Test execution summary"
        );

        for step in &self.step_timings {
            debug!(
                test_name = self.test_name,
                step = step.step,
                duration_ms = step.duration.as_millis(),
                "Step timing detail"
            );
        }
    }

    pub fn was_successful(&self) -> bool {
        matches!(self.final_state, TestState::Complete)
    }

    pub fn get_slowest_step(&self) -> Option<&StepTiming> {
        self.step_timings.iter().max_by_key(|s| s.duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_machine_basic_flow() {
        let mut sm = TestStateMachine::new("test");

        assert_eq!(sm.current_state(), &TestState::Initial);
        assert_eq!(sm.current_state().progress_percentage(), 0);

        sm.transition_to(TestState::ClusterCreating).unwrap();
        assert_eq!(sm.current_state(), &TestState::ClusterCreating);
        assert_eq!(sm.current_state().progress_percentage(), 10);

        sm.complete();
        assert_eq!(sm.current_state(), &TestState::Complete);
        assert_eq!(sm.current_state().progress_percentage(), 100);
        assert!(sm.current_state().is_terminal());
    }

    #[test]
    fn test_state_machine_failure() {
        let mut sm = TestStateMachine::new("test");

        sm.fail_with_error("Test error".to_string());
        assert!(sm.current_state().is_failed());
        assert!(sm.current_state().is_terminal());

        // Cannot transition from failed state
        let result = sm.transition_to(TestState::ClusterCreating);
        assert!(result.is_err());
    }

    #[test]
    fn test_timing_summary() {
        let mut sm = TestStateMachine::new("timing_test");

        sm.transition_to(TestState::ClusterCreating).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        sm.transition_to(TestState::Complete).unwrap();

        let summary = sm.get_timing_summary();
        assert_eq!(summary.test_name, "timing_test");
        assert!(summary.total_duration.as_millis() >= 10);
        assert!(!summary.step_timings.is_empty());
        assert!(summary.was_successful());
    }
}
