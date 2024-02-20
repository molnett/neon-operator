use crate::neon_storage::controller::NeonStorage;
use kube::CustomResourceExt;

pub mod neon_storage;

fn main() {
    print!("{}", serde_yaml::to_string(&NeonStorage::crd()).unwrap())
}
