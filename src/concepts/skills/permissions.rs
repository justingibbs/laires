use super::{Permission, Skills};

impl Skills {
    pub fn set_permission(&mut self, skill_name: &str, permission: Permission) {
        if self.registry.contains_key(skill_name) {
            self.permissions.insert(skill_name.to_string(), permission);
        }
    }

    pub fn is_permitted(&self, skill_name: &str) -> bool {
        match self.permissions.get(skill_name) {
            Some(Permission::Enabled) => true,
            Some(Permission::Disabled) => false,
            Some(Permission::ConditionalOn(cond)) => match cond.as_str() {
                "is_local" => self.provider_is_local,
                "is_cloud" => !self.provider_is_local,
                _ => false,
            },
            None => false,
        }
    }

    pub fn set_provider_locality(&mut self, is_local: bool) {
        self.provider_is_local = is_local;
    }
}
