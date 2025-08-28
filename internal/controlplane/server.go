package controlplane

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"sync"
	"time"

	"k8s.io/client-go/tools/clientcmd"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

func Run(ctx context.Context, log *slog.Logger) error {
	baseK8sConfig, err := clientcmd.BuildConfigFromFlags("", "")
	if err != nil {
		return err
	}

	// Allows the client to refresh the token when it expires
	// if the file exists, use it, otherwise use the bearer token
	if _, err := os.Stat("/var/run/secrets/kubernetes.io/serviceaccount/token"); err == nil {
		baseK8sConfig.BearerTokenFile = "/var/run/secrets/kubernetes.io/serviceaccount/token"
	}

	k8sClient, err := client.New(baseK8sConfig, client.Options{})
	if err != nil {
		return fmt.Errorf("failed to create kubernetes client: %w", err)
	}

	if err := v1alpha1.SchemeBuilder.AddToScheme(k8sClient.Scheme()); err != nil {
		return fmt.Errorf("failed to add scheme: %w", err)
	}

	srv := newServer(
		log,
		k8sClient,
	)

	host := os.Getenv("HTTP_HOST")
	if host == "" {
		host = "0.0.0.0"
	}
	port := os.Getenv("HTTP_PORT")
	if port == "" {
		port = "8081"
	}

	httpServer := &http.Server{
		Addr:    net.JoinHostPort(host, port),
		Handler: srv,
	}

	go func() {
		log.Info(fmt.Sprintf("listening on %s", httpServer.Addr))
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Error("error listening and serving", "error", err)
		}
	}()

	var wg sync.WaitGroup
	wg.Add(1)

	go func() {
		defer wg.Done()
		<-ctx.Done()
		shutdownCtx := context.Background()
		shutdownCtx, cancel := context.WithTimeout(shutdownCtx, 10*time.Second)
		defer cancel()
		if err := httpServer.Shutdown(shutdownCtx); err != nil {
			fmt.Fprintf(os.Stderr, "error shutting down http server: %s\n", err)
		}
	}()
	wg.Wait()

	return nil
}

func newServer(log *slog.Logger, k8sClient client.Client) http.Handler {
	mux := http.NewServeMux()

	addRoutes(mux, log, k8sClient)

	return mux
}

func encode[T any](w http.ResponseWriter, _ *http.Request, status int, v T) error {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(v); err != nil {
		return fmt.Errorf("encode json: %w", err)
	}
	return nil
}

// decode is a utility function for future API endpoints
//
//nolint:unused
func decode[T any](r *http.Request) (T, error) {
	var v T
	if err := json.NewDecoder(r.Body).Decode(&v); err != nil {
		return v, fmt.Errorf("decode json: %w", err)
	}
	return v, nil
}
