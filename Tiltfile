# Build operator image
docker_build('neon-controller:latest', '.', dockerfile='Dockerfile.operator', build_args={'TARGETOS': 'linux', 'TARGETARCH': 'arm64'})
docker_build('neon-controlplane:latest', '.', dockerfile='Dockerfile.controlplane', build_args={'TARGETOS': 'linux', 'TARGETARCH': 'arm64'})

# Deploy operator
k8s_yaml(kustomize('./config/default'))
