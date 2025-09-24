use uuid::Uuid;

use bees_prometheus_exporter::collector::BeesCollector;

#[tokio::test]
async fn test_collect_all_data_from_tests_directory() {
    // Get the tests directory relative to the project root at compile time
    let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");

    // Call collect_all_data on the tests directory
    let result = BeesCollector::collect_all_data(&tests_dir).await;

    // Assert that the call was successful
    assert!(
        result.is_ok(),
        "collect_all_data should succeed: {:?}",
        result
    );

    let data = result.unwrap();

    // We should have some data since there are .status files in the tests directory
    assert!(
        !data.is_empty(),
        "Should have collected data from status files"
    );

    // Check that we can parse the UUIDs from the filenames
    let expected_uuids = vec![
        "0cadef6c-c480-41f2-95b7-511609815820",
        "464d43b3-8362-45b6-8f65-198ac3dcb507",
        "798ca972-f994-46ab-8e1a-9c3a24c92e85",
        "ab0f09d8-cbf5-461b-9068-31d9a69cb163",
    ];

    for uuid_str in expected_uuids {
        let uuid = Uuid::parse_str(uuid_str).expect("Should be valid UUID");
        assert!(
            data.contains_key(&uuid),
            "Should contain data for UUID {}",
            uuid
        );

        let fs_metrics = &data[&uuid];

        // Check that we have some stats and progress data
        assert!(
            !fs_metrics.stats.is_empty(),
            "Should have parsed stats data for UUID {}",
            uuid
        );
        assert!(
            !fs_metrics.progress.is_empty(),
            "Should have parsed progress data for UUID {}",
            uuid
        );

        // Check for at least one metric that should exist in most bees status files
        // Use a more flexible approach since different files may have different metrics
        let has_any_expected_metric = fs_metrics.stats.contains_key("crawl_done")
            || fs_metrics.stats.contains_key("crawl_discard_high")
            || fs_metrics.stats.contains_key("addr_block");

        assert!(
            has_any_expected_metric,
            "Should contain at least one expected metric for UUID {}",
            uuid
        );
    }

    // Verify all data was parsed correctly
    for (uuid, metrics) in &data {
        assert!(
            metrics.timestamp > 0,
            "Should have a valid timestamp for UUID {}",
            uuid
        );

        // Progress data should be structured correctly
        for progress_row in &metrics.progress {
            // Check that progress rows have sensible values
            assert!(
                !progress_row.extsz.is_empty(),
                "Progress row should have extent size"
            );
            // datasz can be 0, gen_min/max should be valid numbers
        }
    }

    println!(
        "Successfully collected and validated data for {} UUIDs",
        data.len()
    );
}
