pub mod keyboard {
    use core::pin::Pin;
    use core::task::{Context, Poll};

    pub struct SimpleTask {
        id: usize,
    }

    impl SimpleTask {
        pub fn new(id: usize) -> Self {
            SimpleTask { id }
        }

        pub fn poll(&mut self) {
            // Simple task implementation
        }
    }

    pub fn print_keypresses() -> SimpleTask {
        SimpleTask::new(1)
    }
}
