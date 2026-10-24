"""Verify release gates with synthetic GitHub responses; no publication occurs."""
import copy
import os
import unittest
from unittest.mock import patch

import package
import release


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.commit = 'a' * 40
        self.run = {'path': '.github/workflows/ci.yml', 'event': 'push', 'head_branch': 'main',
                    'head_sha': self.commit, 'status': 'completed', 'conclusion': 'success',
                    'repository': {'full_name': release.REPOSITORY}}
        names = [f'Validate ({target})' for target in package.TARGETS] + ['PR policy', 'CI']
        self.jobs = [{'name': name, 'status': 'completed', 'conclusion': 'success'} for name in names]
        self.main_commit = self.commit
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {'GITHUB_REPOSITORY': release.REPOSITORY,
                                'GITHUB_REF': 'refs/heads/main', 'GITHUB_SHA': self.commit}, clear=True).start()
        patch.object(package, 'git', return_value=self.commit).start()
        patch.object(package, 'version', return_value='0.1.0').start()
        patch.object(release, 'workflow_output').start()
        self.api = patch.object(release, 'api', side_effect=self.response).start()

    def response(self, path, *args, **kwargs):
        if path == 'git/ref/heads/main':
            return {'object': {'sha': self.main_commit}}
        if path == 'actions/runs/123':
            return self.run
        if path == 'actions/runs/123/jobs?filter=latest&per_page=100':
            return {'jobs': self.jobs}
        raise AssertionError('Unexpected API call: ' + path)

    def test_matching_main_run_passes_without_writes(self):
        self.assertEqual(release.verify_run('123'),
                         {'commit': self.commit, 'version': '0.1.0', 'tag': 'v0.1.0'})
        self.assertTrue(all(len(call.args) == 1 for call in self.api.call_args_list))

    def test_foreign_unfinished_or_different_source_runs_fail(self):
        baseline = copy.deepcopy(self.run)
        for field, value in [('path', '.github/workflows/other.yml'), ('event', 'pull_request'),
                              ('head_branch', 'feature'), ('head_sha', 'b' * 40),
                              ('status', 'in_progress'), ('conclusion', 'failure'),
                              ('repository', {'full_name': 'other/repository'})]:
            with self.subTest(field=field):
                self.run = copy.deepcopy(baseline)
                self.run[field] = value
                with self.assertRaises(ValueError):
                    release.verify_run('123')

    def test_missing_failed_or_skipped_platform_checks_fail(self):
        baseline = copy.deepcopy(self.jobs)
        self.jobs.pop()
        with self.assertRaises(ValueError):
            release.verify_run('123')
        for result in ['failure', 'cancelled', 'skipped', 'neutral']:
            self.jobs = copy.deepcopy(baseline)
            self.jobs[0]['conclusion'] = result
            with self.subTest(result=result), self.assertRaises(ValueError):
                release.verify_run('123')

    def test_main_moving_after_qualification_fails(self):
        self.main_commit = 'b' * 40
        with self.assertRaises(ValueError):
            release.verify_run('123')

    def test_dispatch_from_feature_branch_or_fork_fails(self):
        for name, value in [('GITHUB_REF', 'refs/heads/feature'),
                             ('GITHUB_REPOSITORY', 'other/repository')]:
            with self.subTest(name=name), patch.dict(os.environ, {name: value}):
                with self.assertRaises(ValueError):
                    release.verify_run('123')

    def test_bad_run_ids_fail_before_api_access(self):
        for value in ['', '0', '-1', '123/path', '12\n3']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                release.verify_run(value)
        self.api.assert_not_called()

    def test_published_and_mismatched_drafts_are_not_modified(self):
        source = {'commit': self.commit, 'tag': 'v0.1.0'}
        baseline = {'id': 9, 'draft': True, 'tag_name': 'v0.1.0',
                    'target_commitish': self.commit, 'prerelease': False}
        for field, value in [('draft', False), ('tag_name', 'v0.2.0'),
                              ('target_commitish', 'b' * 40), ('prerelease', True)]:
            draft = {**baseline, field: value}
            with self.subTest(field=field), patch.object(release, 'api', return_value=draft) as api:
                with self.assertRaises(ValueError):
                    release.qualified_release('9', source)
                api.assert_called_once_with('releases/9')

    def test_matching_published_release_is_reconciled_without_another_write(self):
        source = {'commit': self.commit, 'version': '0.1.0', 'tag': 'v0.1.0'}
        published = {'draft': False, 'html_url': 'https://github.com/hudson-frontier/hudson-cli/releases/tag/v0.1.0'}
        with patch.object(release, 'verify_run', return_value=source), \
                patch.object(release, 'compose', return_value={}), \
                patch.object(release, 'qualified_release', return_value=published), \
                patch.object(release, 'verify_assets'), \
                patch.object(release, 'api') as api:
            release.publish(None, '123', '9')
            api.assert_not_called()


if __name__ == '__main__':
    unittest.main()
