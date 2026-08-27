package outbox

import "github.com/prometheus/client_golang/prometheus"

// Metrics holds the Prometheus instruments the worker updates. Create one with
// NewMetrics; pass a nil registerer to get unregistered instruments for tests.
type Metrics struct {
	Sent        prometheus.Counter
	Failed      prometheus.Counter
	Retried     prometheus.Counter
	PollErrors  prometheus.Counter
	BatchSize   prometheus.Histogram
	SendLatency prometheus.Histogram
	Depth       prometheus.Gauge
}

// NewMetrics builds the instrument set under the bowline_notify prefix and
// registers it with reg when reg is not nil.
func NewMetrics(reg prometheus.Registerer) *Metrics {
	m := &Metrics{
		Sent: prometheus.NewCounter(prometheus.CounterOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "sent_total",
			Help: "Notifications accepted by the SMTP server.",
		}),
		Failed: prometheus.NewCounter(prometheus.CounterOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "failed_total",
			Help: "Notifications parked as failed after exhausting their attempts or hitting a permanent error.",
		}),
		Retried: prometheus.NewCounter(prometheus.CounterOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "retried_total",
			Help: "Delivery attempts that failed and were scheduled for another try.",
		}),
		PollErrors: prometheus.NewCounter(prometheus.CounterOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "poll_errors_total",
			Help: "Polls that could not claim a batch, usually because the database was unreachable.",
		}),
		BatchSize: prometheus.NewHistogram(prometheus.HistogramOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "batch_size",
			Help:    "Rows claimed per poll.",
			Buckets: []float64{0, 1, 2, 5, 10, 25, 50, 100, 250},
		}),
		SendLatency: prometheus.NewHistogram(prometheus.HistogramOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "send_duration_seconds",
			Help:    "Wall time of one SMTP delivery attempt, successful or not.",
			Buckets: prometheus.ExponentialBuckets(0.01, 2, 12),
		}),
		Depth: prometheus.NewGauge(prometheus.GaugeOpts{
			Namespace: "bowline", Subsystem: "notify", Name: "outbox_depth",
			Help: "Rows waiting for delivery (status pending or sending), sampled periodically.",
		}),
	}
	if reg != nil {
		reg.MustRegister(m.Sent, m.Failed, m.Retried, m.PollErrors, m.BatchSize, m.SendLatency, m.Depth)
	}
	return m
}
