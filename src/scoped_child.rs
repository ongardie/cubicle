use std::ops::{Deref, DerefMut};
use std::process::{Child, Command};

pub struct ScopedChild(Child);

impl Deref for ScopedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for ScopedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for ScopedChild {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub trait ScopedSpawn {
    fn scoped_spawn(&mut self) -> std::io::Result<ScopedChild>;
}

impl ScopedSpawn for Command {
    fn scoped_spawn(&mut self) -> std::io::Result<ScopedChild> {
        Ok(ScopedChild(self.spawn()?))
    }
}
