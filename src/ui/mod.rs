pub mod keypair;
mod task;

pub use task::*;

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

pub trait Ui: TaskStarter {
    type List: ListView;

    async fn ready(&self);

    fn list<L: ListModel + 'static>(&self, list: L) -> Self::List;
//    async fn ask(&self, prompt: Box<dyn Prompter>);
}
