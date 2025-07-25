IMG = 'molnett/neon-operator'

# Build docker image - Tilt will automatically update the deployment
docker_build(IMG, '.', dockerfile="Dockerfile.multi.optimized")

k8s_yaml('yaml/operator/operator.yaml')
k8s_resource('neon-operator',
    # Optionally add port forwards for debugging
    port_forwards=['8081:8081']
)
