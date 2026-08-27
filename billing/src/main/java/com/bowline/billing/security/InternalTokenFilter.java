package com.bowline.billing.security;

import com.bowline.billing.config.BillingProperties;
import com.bowline.billing.web.ProblemResponse;
import com.bowline.billing.web.RequestIdFilter;
import com.fasterxml.jackson.databind.ObjectMapper;
import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletException;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Set;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.core.Ordered;
import org.springframework.core.annotation.Order;
import org.springframework.http.HttpStatus;
import org.springframework.stereotype.Component;
import org.springframework.web.filter.OncePerRequestFilter;

/**
 * Every route except the probes and the metrics scrape requires
 * {@code X-Internal-Token} to equal {@code INTERNAL_SERVICE_TOKEN}. The comparison is
 * constant time and a failure is answered with a problem+json 401 before any handler runs.
 */
@Component
@Order(Ordered.HIGHEST_PRECEDENCE + 10)
public class InternalTokenFilter extends OncePerRequestFilter {

    public static final String HEADER = "X-Internal-Token";
    static final Set<String> OPEN_PATHS = Set.of("/healthz", "/readyz", "/metrics");

    private static final Logger log = LoggerFactory.getLogger(InternalTokenFilter.class);

    private final byte[] expected;
    private final ObjectMapper mapper;

    public InternalTokenFilter(BillingProperties properties, ObjectMapper mapper) {
        this.expected = properties.internalToken().getBytes(StandardCharsets.UTF_8);
        this.mapper = mapper;
    }

    @Override
    protected boolean shouldNotFilter(HttpServletRequest request) {
        return OPEN_PATHS.contains(request.getRequestURI());
    }

    @Override
    protected void doFilterInternal(HttpServletRequest request, HttpServletResponse response, FilterChain chain)
            throws ServletException, IOException {
        String presented = request.getHeader(HEADER);
        if (presented == null || !MessageDigest.isEqual(expected, presented.getBytes(StandardCharsets.UTF_8))) {
            log.warn("rejected {} {}: missing or invalid internal token", request.getMethod(), request.getRequestURI());
            ProblemResponse problem = ProblemResponse.of(
                    HttpStatus.UNAUTHORIZED,
                    "unauthorized",
                    "A valid X-Internal-Token header is required.",
                    RequestIdFilter.requestId(request));
            response.setStatus(HttpStatus.UNAUTHORIZED.value());
            response.setContentType(ProblemResponse.MEDIA_TYPE);
            response.setCharacterEncoding(StandardCharsets.UTF_8.name());
            mapper.writeValue(response.getOutputStream(), problem);
            return;
        }
        chain.doFilter(request, response);
    }
}
