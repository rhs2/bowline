package com.bowline.billing;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.boot.context.properties.ConfigurationPropertiesScan;

/**
 * Bowline billing service: renders invoice PDFs and customer statements and builds
 * AR aging spreadsheets. It is called only by the API over HTTP with a shared
 * internal token and reads the database through a read-only role.
 */
@SpringBootApplication
@ConfigurationPropertiesScan
public class BillingApplication {

    public static void main(String[] args) {
        SpringApplication.run(BillingApplication.class, args);
    }
}
