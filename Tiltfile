# Build operator image
docker_build('controller:latest', '.')

# Deploy operator
k8s_yaml(kustomize('./config/default'))
