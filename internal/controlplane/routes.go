package controlplane

import (
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"time"

	"oltp.molnett.org/neon-operator/specs/compute"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

func addRoutes(
	mux *http.ServeMux,
	log *slog.Logger,
	k8sClient client.Client,
) {
	mux.Handle("/compute/api/v2/computes/{compute_id}/spec", logRequests(log, handleComputeSpec(log, k8sClient)))
	mux.Handle("/healthz", logRequests(log, handleHealthCheck()))
	mux.Handle("/readyz", logRequests(log, handleHealthCheck()))
	mux.Handle("/notify-attach", logRequests(log, notifyAttach(log, k8sClient)))
}

type responseWriter struct {
	http.ResponseWriter
	status int
}

func (w *responseWriter) WriteHeader(status int) {
	w.status = status
	w.ResponseWriter.WriteHeader(status)
}

func logRequests(log *slog.Logger, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		wrapped := &responseWriter{ResponseWriter: w, status: http.StatusOK}

		next.ServeHTTP(wrapped, r)

		log.Info("request",
			"method", r.Method,
			"path", r.URL.Path,
			"status", wrapped.status,
			"duration_ms", time.Since(start).Milliseconds(),
			"remote_addr", r.RemoteAddr,
			"user_agent", r.UserAgent(),
		)
	})
}

func handleHealthCheck() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
}

func handleComputeSpec(log *slog.Logger, k8sClient client.Client) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {

		computeID := r.PathValue("compute_id")

		spec, err := compute.GenerateComputeSpec(r.Context(), log, k8sClient, nil, computeID)
		if err != nil {
			log.Error("Failed to generate compute spec", "computeID", computeID, "error", err)
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		err = encode(w, r, http.StatusOK, spec)
		if err != nil {
			log.Error("Failed to encode compute spec", "computeID", computeID, "error", err)
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
	})
}

func notifyAttach(log *slog.Logger, k8sClient client.Client) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if err := r.Body.Close(); err != nil {
				log.Error("Failed to close request body", "error", err)
			}
		}()
		ctx := r.Context()
		var failcount int

		// Read full body
		body, err := io.ReadAll(r.Body)
		if err != nil {
			log.Error("Failed to read request body", "error", err)
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		log.Info("Received notify attach request", "request", string(body))

		var actualRequest compute.ComputeHookNotifyRequest
		err = json.Unmarshal(body, &actualRequest)
		if err != nil {
			log.Error("Failed to Unmarshal the request", "error", err)
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Find deployments for the tenant
		deployments, err := compute.FindTenantDeployments(ctx, k8sClient, actualRequest.TenantID)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			log.Error("Failed to find tenant deployments", "error", err)
			return
		}
		if len(deployments.Items) == 0 {
			log.Warn("TenantID not found in any deployment labels", "tenantID", actualRequest.TenantID)
			w.WriteHeader(http.StatusNotFound)
			return
		}

		// Generate compute spec using the deployment
		for _, deployment := range deployments.Items {
			if err := compute.RefreshConfiguration(ctx, log, k8sClient, actualRequest, &deployment); err != nil {
				log.Error("Failed to refresh the configuration", "deployment", deployment.Name, "error", err)
				failcount = failcount + 1
			} else {
				log.Info("Successfully refreshed configuration", "deployment", deployment.Name)
			}
		}

		if failcount > 0 {
			w.WriteHeader(http.StatusInternalServerError)
			log.Error("Failed to refresh configuration for some deployments", "failed_count", failcount, "total_count", len(deployments.Items))
			return
		}

		w.WriteHeader(http.StatusOK)
	})
}
