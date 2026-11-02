
use std::cmp::min;

pub struct Nav
	{
	id       : usize,
	}

impl Nav
	{
	pub fn new() -> Self
		{
		Nav { id: 0 }
		}
		
	pub fn increase_id(&mut self, last :usize)
		{
		self.id += 1;
		self.id = min(self.id, last-1);
		println!("Next {}", self.id);
		}
	
	pub fn decrease_id(&mut self)
		{
		self.id = self.id.saturating_sub(1);
		println!("Previous {}", self.id);
		}
		

	pub fn get_id(&mut self) -> usize
		{
		self.id
		}
	
	}
