use kube::CustomResourceExt;
use neon_cluster::controllers::resources::{NeonBranch, NeonCluster, NeonProject};

fn main() {
    print!("{}", serde_yaml::to_string(&NeonCluster::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&NeonBranch::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&NeonProject::crd()).unwrap());
}
