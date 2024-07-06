use neon_storage::controllers::controller::NeonStorage;
use kube::CustomResourceExt;

fn main() {
    print!("{}", serde_yaml::to_string(&NeonStorage::crd()).unwrap())
}
