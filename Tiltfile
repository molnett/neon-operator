IMG = 'molnett/neon-operator'

# Build docker image - Tilt will automatically update the deployment
docker_build(IMG, '.', dockerfile="Dockerfile.multi", ignore=['target/'])
docker_build('neon-admission-webhook', '.', dockerfile="crates/admission_webhook/Dockerfile", ignore=['target/'])

k8s_yaml('yaml/operator/operator.yaml')
k8s_resource('neon-operator',
    # Optionally add port forwards for debugging
    port_forwards=['8081:8081']
)

k8s_yaml(['yaml/admission/certificate.yaml', 'yaml/admission/deployment.yaml', 'yaml/admission/rbac.yaml', 'yaml/admission/webhook.yaml'])
k8s_resource('neon-admission-webhook')
