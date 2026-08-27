use std::error::Error;

pub mod keypair;

pub trait Task {
    async fn set_message(&self, message: String);
    async fn mark_complete(&self);
    async fn mark_error(&self, message: String);

    async fn start_task(&self, message: String) -> Self;

    fn property_list(&self, props: Vec<(String, String)>);
}

pub trait Progress {
    async fn set_progress(&self, progress: usize);
    async fn set_task(&self, task: &String);
    async fn complete(&self);
}

// pub trait Prompt {
//     async fn choose(&self, choices: Vec<String>) -> Option<u32>;
//     async fn prompt(&self, message: &String, default: &String) -> String;
//     async fn yesno(&self, question: &String) -> bool;
// 
//     async fn add_prompt(&self, prompt: Box<dyn Prompter>);
// }
// 
// pub trait Prompter {
//     async fn start(&self, prompt: Box<dyn Prompt>);
// }

pub trait ListView {
    fn with_options(self, options: &crate::util::ListOptions) -> Self;
    async fn display(&self);
}

pub trait ListModel {
    fn n_rows(&self) -> usize;
    fn column_names(&self) -> Vec<String>;
    fn get(&self, row: usize, column: usize) -> String;
}

/// Types that can be thrown into lists
pub trait ListItem {
    fn column_names() -> &'static [&'static str];
    fn get_field(&self, col: usize) -> String;
}

impl<Item: ListItem> ListModel for Vec<Item> {
    fn n_rows(&self) -> usize { self.len() }
    fn column_names(&self) -> Vec<String> {
        let columns = <Item as ListItem>::column_names();
        columns.to_vec().iter().map(|x| x.to_string()).collect()
    }
    fn get(&self, row: usize, column: usize) -> String {
        self[row].get_field(column)
    }
}

pub trait Ui {
    type TaskHandle: Task + Clone;
    type List: ListView;

    async fn start_task(&self, task: String) -> Self::TaskHandle;

    async fn ready(&self);

    fn list<L: ListModel + 'static>(&self, list: L) -> Self::List;
//    async fn ask(&self, prompt: Box<dyn Prompter>);
}


// Extension

pub trait UiExt : Ui {
    async fn task<A, R, Task>(&self, desc: String,
                              task: Task) -> Result<A, Box<dyn Error>>
    where R: Future<Output = Result<A, Box<dyn Error>>>,
          Task: Fn(Self::TaskHandle) -> R;
}

impl<T: Ui> UiExt for T {
    async fn task<A, R, Task>(&self, desc: String,
                        task: Task) -> Result<A, Box<dyn Error>>
    where R: Future<Output = Result<A, Box<dyn Error>>>,
          Task: Fn(Self::TaskHandle) -> R {
        let running_task = self.start_task(desc).await;
        match task(running_task.clone()).await {
            Err(e) => {
                running_task.mark_error(format!("{}", e).into()).await;
                Err(e)
            },
            Ok(x) => {
                running_task.mark_complete().await;
                Ok(x)
            }
        }
    }
}
