use kube::CustomResourceExt as _;
use neon_cluster::api::v1::{neonbranch::NeonBranch, neoncluster::NeonCluster, neonproject::NeonProject};
use neon_cluster::api::v1alpha1::neonpageserver::NeonPageserver;

fn main() {
    print!("{}", serde_yaml::to_string(&NeonCluster::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&NeonPageserver::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&NeonBranch::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&NeonProject::crd()).unwrap());
}
