use crate::neon_cluster::controller::NeonCluster;
use kube::CustomResourceExt;

pub mod neon_cluster;
mod util;

fn main() {
    print!("{}", serde_yaml::to_string(&NeonCluster::crd()).unwrap())
}
