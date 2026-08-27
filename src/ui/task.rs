use std::error::Error;
use std::future::Future;

/// Something that can start a task.
///
/// A UI starts root tasks. A task starts child tasks.
pub trait TaskStarter {
    type TaskHandle: Task + Clone;

    async fn start_task(&self, message: String) -> Self::TaskHandle;
}

/// A running task exposed by a UI backend.
pub trait Task: TaskStarter<TaskHandle = Self> + Send + Sync + Clone {
    async fn set_message(&self, message: String);
    async fn mark_complete(&self);
    async fn mark_error(&self, message: String);

    fn property_list(&self, props: Vec<(String, String)>);
}

pub trait Progress {
    async fn set_progress(&self, progress: usize);
    async fn set_task(&self, task: &String);
    async fn complete(&self);
}

/// Convenience operations available to every task starter.
pub trait TaskStarterExt: TaskStarter {
    async fn task<A, R, Operation>(
        &self,
        description: String,
        operation: Operation,
    ) -> Result<A, Box<dyn Error>>
    where
        R: Future<Output = Result<A, Box<dyn Error>>>,
        Operation: FnOnce(Self::TaskHandle) -> R;
}

impl<T: TaskStarter> TaskStarterExt for T {
    async fn task<A, R, Operation>(
        &self,
        description: String,
        operation: Operation,
    ) -> Result<A, Box<dyn Error>>
    where
        R: Future<Output = Result<A, Box<dyn Error>>>,
        Operation: FnOnce(Self::TaskHandle) -> R,
    {
        let running_task = self.start_task(description).await;
        match operation(running_task.clone()).await {
            Err(error) => {
                running_task.mark_error(error.to_string()).await;
                Err(error)
            }
            Ok(value) => {
                running_task.mark_complete().await;
                Ok(value)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TaskId(usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TaskPlacement {
    pub id: TaskId,
    pub index: usize,
    pub depth: usize,
}

#[derive(Debug)]
struct TaskNode {
    id: TaskId,
    parent: Option<TaskId>,
    depth: usize,
}

/// Maintains tasks in depth-first display order.
///
/// New children are inserted at the end of their parent's current subtree, so
/// siblings remain ordered by creation time and all descendants remain grouped
/// beneath their parent.
#[derive(Debug, Default)]
pub(crate) struct TaskTree {
    nodes: Vec<TaskNode>,
    next_id: usize,
}

impl TaskTree {
    pub fn insert(&mut self, parent: Option<TaskId>) -> TaskPlacement {
        let (index, depth) = match parent {
            None => (self.nodes.len(), 0),
            Some(parent) => {
                let parent_index = self
                    .nodes
                    .iter()
                    .position(|node| node.id == parent)
                    .expect("parent task must belong to this task tree");
                let parent_depth = self.nodes[parent_index].depth;
                let mut index = parent_index + 1;
                while index < self.nodes.len()
                    && self.is_descendant_of(self.nodes[index].id, parent)
                {
                    index += 1;
                }
                (index, parent_depth + 1)
            }
        };

        let id = TaskId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(index, TaskNode { id, parent, depth });

        TaskPlacement { id, index, depth }
    }

    fn is_descendant_of(&self, mut node: TaskId, ancestor: TaskId) -> bool {
        loop {
            let Some(parent) = self
                .nodes
                .iter()
                .find(|candidate| candidate.id == node)
                .and_then(|candidate| candidate.parent)
            else {
                return false;
            };

            if parent == ancestor {
                return true;
            }
            node = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_appended() {
        let mut tree = TaskTree::default();
        assert_eq!(tree.insert(None).index, 0);
        assert_eq!(tree.insert(None).index, 1);
    }

    #[test]
    fn children_are_inserted_after_the_parent_subtree() {
        let mut tree = TaskTree::default();
        let root = tree.insert(None);
        let first_child = tree.insert(Some(root.id));
        let grandchild = tree.insert(Some(first_child.id));
        let second_child = tree.insert(Some(root.id));

        assert_eq!(root.depth, 0);
        assert_eq!(first_child.depth, 1);
        assert_eq!(grandchild.depth, 2);
        assert_eq!(second_child.depth, 1);
        assert_eq!(second_child.index, 3);
    }

    #[test]
    fn a_child_can_be_inserted_before_a_later_root() {
        let mut tree = TaskTree::default();
        let first_root = tree.insert(None);
        tree.insert(None);

        let child = tree.insert(Some(first_root.id));
        assert_eq!(child.index, 1);
    }
}
