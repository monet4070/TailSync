import unittest
from check_macos_deployment import minimums, version

class DeploymentGuardTests(unittest.TestCase):
    def test_mixed_load_commands_use_only_deployment_version(self):
        self.assertEqual(minimums('''Load command 0
          cmd LC_SOURCE_VERSION
          version 999.1
        Load command 1
          cmd LC_BUILD_VERSION
          platform 1
          minos 13.0
          sdk 18.0
        Load command 2
          cmd LC_VERSION_MIN_MACOSX
          version 12.0
          sdk 18.0
        '''), ['13.0','12.0'])
    def test_host_minimum_exceeds_product(self):
        self.assertGreater(version(minimums('Load command 0\ncmd LC_BUILD_VERSION\n minos 18.0\n')[0]),version('13.0'))
    def test_missing_minimum_fails_closed(self):
        with self.assertRaises(ValueError): minimums('Load command 0\ncmd LC_SOURCE_VERSION\nversion 13.0\n')
