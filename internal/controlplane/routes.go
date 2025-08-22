package controlplane

import (
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
