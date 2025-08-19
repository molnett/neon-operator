package utils

type contextKey string

const (
	ClusterNameKey    contextKey = "cluster"
	SafekeeperNameKey contextKey = "safekeeper"
	PageserverNameKey contextKey = "pageserver"
	ProjectNameKey    contextKey = "project"
)
