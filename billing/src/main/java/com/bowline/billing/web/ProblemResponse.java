package com.bowline.billing.web;

import java.util.List;
import org.springframework.http.HttpStatus;

/**
 * RFC 7807 problem document with the same shape the API uses: a stable machine
 * readable {@code code}, the request id, and field errors for validation failures.
 * Serialised with snake_case keys ({@code request_id}).
 */
public record ProblemResponse(
        String type,
        String title,
        int status,
        String detail,
        String code,
        String requestId,
        List<FieldError> errors) {

    public static final String MEDIA_TYPE = "application/problem+json";

    /** One invalid field: {@code field} is a dotted path into the request. */
    public record FieldError(String field, String message) {}

    public static ProblemResponse of(HttpStatus status, String code, String detail, String requestId) {
        return new ProblemResponse("about:blank", status.getReasonPhrase(), status.value(), detail, code, requestId, null);
    }

    public static ProblemResponse validation(String detail, String requestId, List<FieldError> errors) {
        HttpStatus status = HttpStatus.UNPROCESSABLE_ENTITY;
        return new ProblemResponse(
                "about:blank", status.getReasonPhrase(), status.value(), detail, "validation_failed", requestId, errors);
    }
}
