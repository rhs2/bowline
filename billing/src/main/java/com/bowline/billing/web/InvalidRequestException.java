package com.bowline.billing.web;

import java.util.List;

/**
 * A request that is well formed but fails a business rule (cross-field checks that bean
 * validation cannot express). Rendered as a 422 {@code validation_failed} problem.
 */
public class InvalidRequestException extends RuntimeException {

    private final transient List<ProblemResponse.FieldError> errors;

    public InvalidRequestException(String message, List<ProblemResponse.FieldError> errors) {
        super(message);
        this.errors = List.copyOf(errors);
    }

    public InvalidRequestException(String field, String message) {
        this(message, List.of(new ProblemResponse.FieldError(field, message)));
    }

    public List<ProblemResponse.FieldError> errors() {
        return errors;
    }
}
