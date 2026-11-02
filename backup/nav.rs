
use std::cmp::min;

pub struct Nav
	{
	state : bool,
	id    : usize,
	}

impl Nav
	{
	pub fn new() -> Self
		{
		Nav { state: false, id: 0}
		}
		
	pub fn increase_id(&mut self, last :usize)
		{
		if self.state
			{
			self.id += 1;
			self.id = min(self.id, last-1);
			println!("Next {}", self.id);
			}
		else{
			println!("Nav off");
			}
		}
	
	pub fn decrease_id(&mut self)
		{
		if self.state
			{
			self.id = self.id.saturating_sub(1);
			println!("Previous {}", self.id);
			}
		else{
			println!("Nav off");
			}
		}
		
	pub fn state(&mut self, state :bool)
		{
		self.state = state;
		self.id = 0;
		println!("NAV {}",self.state);
		}
	
	pub fn get_id(&mut self) -> usize
		{
		self.id
		}
	
	pub fn get_state(&mut self) -> bool
		{
		self.state
		}
	}
